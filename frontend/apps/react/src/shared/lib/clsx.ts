/** Минимальный склейщик классов — чтобы не тянуть зависимость. */
export default function clsx(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(' ');
}
