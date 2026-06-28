import { create } from 'zustand';

export type ToastSeverity = 'error' | 'warn' | 'info';

export interface Toast {
  id: string;
  severity: ToastSeverity;
  message: string;
}

let toastSeq = 0;
const nextToastId = (): string => String(++toastSeq);

interface ToastState {
  toasts: Toast[];
  /** Push a toast. id is auto-assigned. */
  addToast: (severity: ToastSeverity, message: string) => void;
  /** Remove the toast with the given id. */
  dismissToast: (id: string) => void;
}

export const useToastStore = create<ToastState>((set) => ({
  toasts: [],
  addToast: (severity, message) =>
    set((s) => ({ toasts: [...s.toasts, { id: nextToastId(), severity, message }] })),
  dismissToast: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
}));
