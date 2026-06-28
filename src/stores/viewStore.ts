import { create } from 'zustand';

/** 应用顶层视图。扩此联合型即可加新视图。 */
export type View = 'chat' | 'settings';

interface ViewState {
  view: View;
  previous: View;
  /** 切到 v，记住当前为 previous（供 goBack）。 */
  navigate: (v: View) => void;
  /** 回到 previous 视图。 */
  goBack: () => void;
}

export const useViewStore = create<ViewState>((set) => ({
  view: 'chat',
  previous: 'chat',
  navigate: (v) => set((s) => ({ previous: s.view, view: v })),
  goBack: () => set((s) => ({ view: s.previous })),
}));
