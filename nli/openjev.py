# Взято из AlexWortega/openjev (modeling_openjev.py, класс OpenJevCrossEncoder),
# https://huggingface.co/AlexWortega/openjev, лицензия MIT (указана в карточке
# модели). Оставлен только кросс-энкодер: голова LatentMLPHead тренажёру не
# нужна. Код класса не менялся, кроме удаления методов rerank и grade.
from __future__ import annotations

import numpy as np
import torch

CON, ENT, NEU = 0, 1, 2
DEFAULT_TEMPLATE = "Premise: {premise}\nHypothesis: {hypothesis}"


class OpenJevCrossEncoder:
    def __init__(self, path: str, subfolder: str | None = None, device: str | None = None, dtype=torch.bfloat16,
                 bs: int = 32, max_len: int = 4096):
        from transformers import AutoModelForSequenceClassification, AutoTokenizer

        kw = {"subfolder": subfolder} if subfolder else {}
        self.tok = AutoTokenizer.from_pretrained(path, **kw)
        self.model = AutoModelForSequenceClassification.from_pretrained(path, dtype=dtype, **kw)
        self.device = device or ("cuda" if torch.cuda.is_available() else "cpu")
        self.model.to(self.device).eval()
        self.template = getattr(self.model.config, "nli_template", None) or DEFAULT_TEMPLATE
        if self.tok.pad_token is None:
            self.tok.pad_token = self.tok.eos_token
        self.tok.padding_side = "right"  # the head pools the last non-pad token
        tc = self.model.config.get_text_config()
        if tc.pad_token_id is None:
            tc.pad_token_id = self.tok.pad_token_id
        self.backbone = getattr(self.model, self.model.base_model_prefix)
        self.bs, self.max_len = bs, max_len

    def _encode(self, pairs):
        texts = [self.template.format(premise=p.strip(), hypothesis=h.strip()) for p, h in pairs]
        enc = self.tok(texts, truncation=True, max_length=self.max_len, padding=True, return_tensors="pt")
        return {k: v.to(self.device) for k, v in enc.items()}

    @torch.no_grad()
    def _pooled(self, enc):
        h = self.backbone(**enc).last_hidden_state
        last = enc["attention_mask"].sum(1) - 1
        return h[torch.arange(h.shape[0], device=h.device), last]

    @torch.no_grad()
    def latents(self, pairs) -> np.ndarray:
        """Pooled last-token hidden state (the input of the `score` head) for each (premise, hypothesis) pair."""
        out = []
        for i in range(0, len(pairs), self.bs):
            out.append(self._pooled(self._encode(pairs[i:i + self.bs])).float().cpu().numpy())
        return np.concatenate(out, 0)

    @torch.no_grad()
    def predict(self, pairs) -> np.ndarray:
        """Softmax probabilities [contradiction, entailment, neutral] per pair."""
        out = []
        for i in range(0, len(pairs), self.bs):
            logits = self.model.score(self._pooled(self._encode(pairs[i:i + self.bs]))).float()
            out.append(torch.softmax(logits, -1).cpu().numpy())
        return np.concatenate(out, 0)

    @torch.no_grad()
    def _pooled_hypotheses(self, premise: str, hypotheses) -> torch.Tensor:
        """Score hypotheses with one shared text prefix and a batched cached continuation.

        Qwen3.5 mixes full and recurrent linear attention. A packed 4D tree mask
        cannot isolate the recurrent layers, so their state and the full-attention
        KV cache are branched into the batch after a single prefix prefill.
        """
        hypotheses = list(hypotheses)
        if not hypotheses:
            raise ValueError("hypotheses must not be empty")
        pairs = [(premise, h) for h in hypotheses]
        # A second forward and cache branching are unlikely to pay off for
        # one or two hypotheses; use the existing batched path instead.
        if len(pairs) < 3:
            return self._pooled(self._encode(pairs))

        # Tokenize complete inputs first: a tokenizer can merge characters at
        # the premise/hypothesis boundary, so a textual split is not reliable.
        texts = [self.template.format(premise=p.strip(), hypothesis=h.strip()) for p, h in pairs]
        sequences = [self.tok(t, truncation=True, max_length=self.max_len)["input_ids"] for t in texts]
        common = 0
        for column in zip(*sequences):
            if len(set(column)) != 1:
                break
            common += 1
        common = min(common, min(map(len, sequences)) - 1)
        if common < 1:
            return self._pooled(self._encode(pairs))

        prefix = torch.tensor([sequences[0][:common]], device=self.device)
        prefix_positions = torch.arange(common, device=self.device).unsqueeze(0)
        cache = self.backbone(input_ids=prefix, position_ids=prefix_positions, use_cache=True).past_key_values
        if cache is None:
            raise RuntimeError("Qwen3.5 did not return a cache for shared-prefix inference")
        cache.reorder_cache(torch.zeros(len(sequences), dtype=torch.long, device=self.device))

        suffixes = [s[common:] for s in sequences]
        lengths = torch.tensor([len(s) for s in suffixes], device=self.device)
        width = int(lengths.max())
        input_ids = torch.full((len(suffixes), width), self.tok.pad_token_id,
                               dtype=torch.long, device=self.device)
        for row, suffix in enumerate(suffixes):
            input_ids[row, :len(suffix)] = torch.tensor(suffix, device=self.device)
        offsets = torch.arange(width, device=self.device).unsqueeze(0)
        valid = offsets < lengths.unsqueeze(1)
        attention_mask = torch.cat((torch.ones((len(suffixes), common), device=self.device,
                                               dtype=torch.long), valid.long()), dim=1)
        position_ids = (common + offsets).expand(len(suffixes), -1)
        hidden = self.backbone(input_ids=input_ids, attention_mask=attention_mask,
                               position_ids=position_ids, past_key_values=cache,
                               use_cache=True).last_hidden_state
        return hidden[torch.arange(len(suffixes), device=self.device), lengths - 1]

    @torch.no_grad()
    def latents_hypotheses(self, premise: str, hypotheses) -> np.ndarray:
        """Pooled last-token latents for hypotheses sharing one premise."""
        return self._pooled_hypotheses(premise, hypotheses).float().cpu().numpy()

    @torch.no_grad()
    def predict_hypotheses(self, premise: str, hypotheses) -> np.ndarray:
        """NLI probabilities for one premise and many hypotheses, sharing the prefix."""
        return torch.softmax(self.model.score(self._pooled_hypotheses(premise, hypotheses)).float(), -1).cpu().numpy()
