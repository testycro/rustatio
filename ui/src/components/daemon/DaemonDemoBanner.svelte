<script>
  import { AlertTriangle } from '@lucide/svelte';
  import { getRunMode } from '$lib/api.js';
  import { daemonI18n } from '$lib/daemon/i18n.svelte.js';

  // Shown only in the browser-only (WASM) build, where the Daemon view is a simulation:
  // there is no backend, so no real daemon is running behind it.
  const TEXT = {
    fr: {
      badge: 'DÉMO',
      title: 'Démonstration : aucun daemon connecté',
      body: "Ce panneau est une simulation qui tourne dans votre navigateur. Les logs, les règles, le watcher et la configuration sont fictifs : rien n'est enregistré (tout est perdu au rechargement de la page) et rien n'est appliqué à vos torrents. Pour utiliser le vrai daemon, lancez Rustatio avec Docker (mode serveur) ou avec l'application desktop.",
    },
    en: {
      badge: 'DEMO',
      title: 'Demonstration: no daemon connected',
      body: 'This panel is a simulation running in your browser. Logs, rules, watcher and settings are fictional: nothing is saved (everything is lost when the page is reloaded) and nothing is applied to your torrents. To use the real daemon, run Rustatio with Docker (server mode) or with the desktop app.',
    },
  };

  const badgeClass =
    'border border-stat-ratio/40 px-1 text-[0.5625rem] font-semibold tracking-wider text-stat-ratio';

  let text = $derived(TEXT[daemonI18n.lang] ?? TEXT.en);
</script>

{#if getRunMode() === 'wasm'}
  <div
    role="note"
    class="flex items-start gap-2 border border-stat-ratio/40 bg-stat-ratio/10 px-2.5 py-2 text-xs"
  >
    <AlertTriangle size={14} class="mt-0.5 flex-shrink-0 text-stat-ratio" />
    <div class="min-w-0 space-y-0.5">
      <div class="flex items-center gap-1.5">
        <span class={badgeClass}>{text.badge}</span>
        <span class="font-semibold text-foreground">{text.title}</span>
      </div>
      <p class="text-muted-foreground">{text.body}</p>
    </div>
  </div>
{/if}
