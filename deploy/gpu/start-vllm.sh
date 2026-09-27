#!/bin/bash
# Профиль GPU: Qwen3.8-27B (INT4, W4A16) в vLLM на одной RTX 4090 (24 ГБ).
#
# Тренажёр ходит к модели по OpenAI-совместимому API, поэтому переход
# с процессорного профиля — одна переменная: ML_BASE_URL=http://<эта машина>:8091.
#
# Флаги подобраны на RTX 4090 с драйвером 550 (CUDA 12.4), vLLM 0.30.0+cu129:
#   --max-num-seqs 8        у гибридной модели состояние на каждый запрос;
#                           при умолчании (256) кэш не помещается в 24 ГБ
#   --attention-backend TRITON_ATTN, VLLM_USE_FLASHINFER_SAMPLER=0
#                           FlashInfer собирает ядра на лету системным nvcc,
#                           а 12.4 его не собирает; Triton — собирает
#   --language-model-only   зрительный кодировщик не нужен
#   enable_thinking=false   наши запросы — короткие JSON по схеме
#
# Переменные: MODEL_DIR (веса RedHatAI/Qwen3.8-27B-INT4), VLLM (путь к vllm),
# GPU, PORT, CTX, SEQS.
set -e
export CUDA_VISIBLE_DEVICES=${GPU:-0}
export VLLM_USE_FLASHINFER_SAMPLER=0
export PYTORCH_CUDA_ALLOC_CONF=expandable_segments:True
exec ${VLLM:-vllm} serve "${MODEL_DIR:?укажите MODEL_DIR — каталог весов}" \
  --served-model-name local \
  --host ${HOST:-0.0.0.0} --port ${PORT:-8091} \
  --max-model-len ${CTX:-8192} \
  --gpu-memory-utilization 0.92 \
  --language-model-only \
  --attention-backend ${ATTN:-TRITON_ATTN} \
  --max-num-seqs ${SEQS:-8} \
  --default-chat-template-kwargs '{"enable_thinking": false}' \
  "$@"
