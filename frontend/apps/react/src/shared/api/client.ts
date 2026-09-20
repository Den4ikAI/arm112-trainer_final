/**
 * Клиент HTTP. Один вход в сервер на всё приложение.
 *
 * Токен живёт в localStorage: перезагрузка страницы посреди занятия
 * не должна выкидывать курсанта на экран входа.
 */

const TOKEN_KEY = 'arm112.token';

export const getToken = (): string | null => localStorage.getItem(TOKEN_KEY);
export const setToken = (t: string | null): void => {
  if (t) localStorage.setItem(TOKEN_KEY, t);
  else localStorage.removeItem(TOKEN_KEY);
};

/** Ошибка с кодом из тела ответа — коды перечислены в BACKEND.md. */
export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    readonly detail?: string,
    /** Для status_not_allowed — что сервер разрешает вместо присланного. */
    readonly allowed?: string[],
  ) {
    super(detail ?? code);
  }
}

const request = async <T>(
  method: string,
  path: string,
  body?: unknown,
): Promise<T> => {
  const token = getToken();
  const res = await fetch(path, {
    method,
    headers: {
      ...(body === undefined ? {} : { 'Content-Type': 'application/json' }),
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });

  if (res.status === 204) return undefined as T;

  const text = await res.text();
  const data = text ? JSON.parse(text) : null;

  if (!res.ok) {
    // Протухший токен — не ошибка приложения, а повод войти заново
    if (res.status === 401) setToken(null);
    throw new ApiError(res.status, data?.error ?? 'unknown', data?.detail, data?.allowed);
  }
  return data as T;
};

export const api = {
  get: <T>(path: string) => request<T>('GET', path),
  post: <T>(path: string, body?: unknown) => request<T>('POST', path, body),
  patch: <T>(path: string, body?: unknown) => request<T>('PATCH', path, body),
  del: <T>(path: string) => request<T>('DELETE', path),
};
