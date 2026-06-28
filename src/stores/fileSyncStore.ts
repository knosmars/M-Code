import { create } from 'zustand';
import { typedInvoke } from '../utils/ipc';

interface FileInterest {
  system: string;
  file: string;
}


interface SyncNotification {
  file: string;
  action: string;
  source: string;
  target_system: string;
  message: string;
}

interface StoredNotification extends SyncNotification {
  id: string;
}

let notificationSeq = 0;
const nextNotificationId = (): string => String(++notificationSeq);

interface FileSyncState {
  interests: FileInterest[];
  notifications: StoredNotification[];
  registerInterest: (system: string, file: string) => Promise<void>;
  unregisterInterest: (system: string, file: string) => Promise<void>;
  publishEvent: (file: string, action: string, source: string) => Promise<SyncNotification[]>;
  checkInterest: (file: string, excludeSystem: string) => Promise<string[]>;
  clearSystem: (system: string) => Promise<void>;
  addNotification: (notification: SyncNotification) => void;
  dismissNotification: (id: string) => void;
}

export const useFileSyncStore = create<FileSyncState>((set, get) => ({
  interests: [],
  notifications: [],

  registerInterest: async (system: string, file: string) => {
    try {
      await typedInvoke<string>('tool_file_sync_register', { system, file });
      set((state) => ({
        interests: [...state.interests, { system, file }],
      }));
    } catch (e) {
      console.error('Failed to register file interest:', e);
    }
  },

  unregisterInterest: async (system: string, file: string) => {
    try {
      await typedInvoke<string>('tool_file_sync_unregister', { system, file });
      set((state) => ({
        interests: state.interests.filter(
          (i) => !(i.system === system && i.file === file)
        ),
      }));
    } catch (e) {
      console.error('Failed to unregister file interest:', e);
    }
  },

  publishEvent: async (file: string, action: string, source: string) => {
    try {
      const notifications = await typedInvoke<SyncNotification[]>(
        'tool_file_sync_publish',
        { file, action, source }
      );

      notifications.forEach((n) => {
        get().addNotification(n);
      });

      return notifications;
    } catch (e) {
      console.error('Failed to publish file event:', e);
      return [];
    }
  },

  checkInterest: async (file: string, excludeSystem: string) => {
    try {
      return await typedInvoke<string[]>('tool_file_sync_check', {
        file,
        excludeSystem,
      });
    } catch (e) {
      console.error('Failed to check file interest:', e);
      return [];
    }
  },

  clearSystem: async (system: string) => {
    try {
      await typedInvoke<string>('tool_file_sync_clear', { system });
      set((state) => ({
        interests: state.interests.filter((i) => i.system !== system),
      }));
    } catch (e) {
      console.error('Failed to clear system interests:', e);
    }
  },

  addNotification: (notification: SyncNotification) => {
    set((state) => ({
      notifications: [...state.notifications, { ...notification, id: nextNotificationId() }],
    }));
  },

  dismissNotification: (id: string) => {
    set((state) => ({
      notifications: state.notifications.filter((n) => n.id !== id),
    }));
  },
}));
