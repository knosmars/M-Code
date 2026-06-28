import { useEffect } from 'react';
import { ChatWindow } from './components/ChatWindow';
import { SettingsPanel } from './components/SettingsPanel';
import { useProviderStore } from './stores/providerStore';
import { useViewStore } from './stores/viewStore';

function App() {
  const view = useViewStore((s) => s.view);
  const initialize = useProviderStore((s) => s.initialize);

  useEffect(() => {
    initialize();
  }, [initialize]);

  switch (view) {
    case 'settings':
      return <SettingsPanel />;
    default:
      return <ChatWindow />;
  }
}

export default App;
