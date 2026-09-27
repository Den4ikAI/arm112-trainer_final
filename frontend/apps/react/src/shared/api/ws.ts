/**
 * Канал класса.
 *
 * Держит одно соединение на всё приложение и сам его восстанавливает:
 * занятие идёт час, за это время сеть на учебном стенде успевает
 * моргнуть не раз, а курсант не должен замечать этого вовсе.
 *
 * Канал — ускорение, а не источник истины. Данные всегда перечитываются
 * через REST: событие только говорит, что пора перечитать. Иначе после
 * любого разрыва интерфейс показывал бы устаревшую ленту.
 */
import { getToken } from './client';

export interface ServerEvent {
  type: string;
  [key: string]: unknown;
}

type Listener = (event: ServerEvent) => void;

/** Пауза перед переподключением: растёт до минуты, чтобы не долбить
 *  упавший сервер, но первые попытки идут почти сразу. */
const BACKOFF_MS = [500, 1000, 2000, 5000, 10_000, 30_000, 60_000];

class ClassChannel {
  private socket: WebSocket | null = null;
  private listeners = new Set<Listener>();
  private topics = new Set<string>();
  private attempt = 0;
  private timer: ReturnType<typeof setTimeout> | null = null;
  /** Закрыли нарочно — переподключаться не надо. */
  private closed = false;
  /** Соединение рвалось: после восстановления события за обрыв потеряны. */
  private lost = false;

  constructor() {
    // Браузер знает, что сеть вернулась, раньше, чем истечёт пауза:
    // после обрыва в полминуты очередная попытка по расписанию была бы
    // только через 30 секунд. Не ждём её
    if (typeof window !== 'undefined') {
      window.addEventListener('online', () => this.wake());
    }
  }

  /** Подписаться на события. Возвращает функцию отписки. */
  on(listener: Listener): () => void {
    this.listeners.add(listener);
    return () => { this.listeners.delete(listener); };
  }

  /** Добавить топики. Соединение поднимается при первом вызове. */
  subscribe(topics: string[]): void {
    const added = topics.filter((t) => !this.topics.has(t));
    topics.forEach((t) => this.topics.add(t));

    if (!this.socket) {
      this.connect();
      return;
    }
    if (added.length && this.socket.readyState === WebSocket.OPEN) {
      this.send({ type: 'subscribe', topics: added });
    }
  }

  unsubscribe(topics: string[]): void {
    topics.forEach((t) => this.topics.delete(t));
    if (this.socket?.readyState === WebSocket.OPEN) {
      this.send({ type: 'unsubscribe', topics });
    }
  }

  /** Закрыть насовсем — при выходе из системы. */
  close(): void {
    this.closed = true;
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    this.topics.clear();
    this.socket?.close();
    this.socket = null;
  }

  private send(message: unknown): void {
    this.socket?.send(JSON.stringify(message));
  }

  private connect(): void {
    const token = getToken();
    // Без токена канал не открыть: сервер его проверяет при апгрейде
    if (!token || this.socket) return;

    this.closed = false;
    const scheme = location.protocol === 'https:' ? 'wss' : 'ws';
    const socket = new WebSocket(
      `${scheme}://${location.host}/ws?token=${encodeURIComponent(token)}`,
    );
    this.socket = socket;

    socket.onopen = () => {
      this.attempt = 0;
      if (this.topics.size) {
        this.send({ type: 'subscribe', topics: [...this.topics] });
      }
      // Пока канала не было, события шли мимо. Для подписчиков это то же,
      // что отставание: пусть перечитают состояние через REST
      if (this.lost) {
        this.lost = false;
        this.listeners.forEach((l) => l({ type: 'lagged', missed: null }));
      }
    };

    socket.onmessage = (e) => {
      let event: ServerEvent;
      try {
        event = JSON.parse(String(e.data));
      } catch {
        return;
      }
      // Сервер честно говорит, что мы отстали и часть событий потеряна.
      // Подписчикам это тот же сигнал «перечитай состояние»
      this.listeners.forEach((l) => l(event));
    };

    socket.onclose = () => {
      this.socket = null;
      if (!this.closed) {
        this.lost = true;
        this.reconnect();
      }
    };

    // onerror приходит перед onclose, отдельной обработки не требует
    socket.onerror = () => socket.close();
  }

  /** Сеть вернулась: переподключиться сейчас, не дожидаясь паузы.
   *  Только если канал ждёт очередной попытки — сами его не открываем */
  private wake(): void {
    if (this.closed || this.socket || !this.timer) return;
    clearTimeout(this.timer);
    this.timer = null;
    this.attempt = 0;
    this.connect();
  }

  private reconnect(): void {
    const delay = BACKOFF_MS[Math.min(this.attempt, BACKOFF_MS.length - 1)];
    this.attempt += 1;
    if (this.timer) clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      this.timer = null;
      this.connect();
    }, delay);
  }
}

export const channel = new ClassChannel();
