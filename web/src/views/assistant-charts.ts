// The charts views lend to the assistant's conversation. Imported once at startup
// (src/main.tsx) so `show_chart` can draw them whichever view is open; each chart is
// its own lazy chunk.
import { registerAssistantChart } from '@/assistant/charts';

registerAssistantChart('fan', () => import('./overview/compact/Fan'));
registerAssistantChart('fi_age', () => import('./overview/compact/FiAge'));
registerAssistantChart('status_stack', () => import('./overview/compact/StatusStack'));
registerAssistantChart('compare_delta', () => import('./scenarios/compact/CompareDelta'));
registerAssistantChart('tornado', () => import('./what-matters/compact/Tornado'));
registerAssistantChart('heatmap', () => import('./what-matters/compact/Heatmap'));
registerAssistantChart('cash_flow', () => import('./cash-flow/compact/CashFlow'));
registerAssistantChart('life_story', () => import('./life-stories/compact/LifeStory'));
