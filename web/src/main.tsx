import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import './styles/index.css';
import { App, appReady } from './App';
import { installDiagnostics } from './state/diagnostics';
import './views/assistant-charts';
import { registerWebMcp } from './webmcp/register';

installDiagnostics();
// Offers the app's tools to an AI agent in the browser, where the browser supports WebMCP. A no-op elsewhere.
registerWebMcp({ ready: appReady });

createRoot(document.getElementById('root') as HTMLElement).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
