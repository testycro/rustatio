<script>
  import { getRunMode } from '$lib/api.js';
  import './daemon.css';
  import { cn } from '$lib/core/utils.js';
  import { feedback, resolveConfirm } from '$lib/daemon/feedback.svelte.js';
  import { daemonI18n, setDaemonLang, t } from '$lib/daemon/i18n.svelte.js';
  import { field } from '$lib/daemon/ui.js';
  import ConfirmDialog from '../common/ConfirmDialog.svelte';
  import DaemonControl from './DaemonControl.svelte';
  import DaemonDemoBanner from './DaemonDemoBanner.svelte';
  import DaemonLogs from './DaemonLogs.svelte';
  import DaemonRules from './DaemonRules.svelte';
  import DaemonWatcher from './DaemonWatcher.svelte';
  import DaemonDesktopControl from './DaemonDesktopControl.svelte';

  // The admin daemon (rustatio_daemon.py) runs next to the server, or inside the desktop app.
  // Everything here talks to it through the server's authenticated proxy (/api/daemon-admin)
  // or the desktop app, so there is no second port and no second login.
  const isServerRuntime = getRunMode() === 'server' || getRunMode() === 'desktop';

  // Desktop app: the daemon is a process of the app that can be stopped, the sections below
  // only make sense while it answers.
  const isDesktop = getRunMode() === 'desktop';
  let desktopReady = $state(false);
  let daemonError = $state('');

  const noticeTones = {
    success: 'border-stat-upload/40 text-stat-upload',
    error: 'border-stat-danger/40 text-stat-danger',
    info: 'border-border text-foreground',
  };
</script>

{#if !isServerRuntime && getRunMode() !== 'wasm'}
  <div class="p-3 text-xs text-muted-foreground">{t('server_only')}</div>
{:else}
  <div class="daemon-root mx-auto max-w-7xl space-y-3">
    <DaemonDemoBanner />

    <div class="flex items-center justify-between gap-2">
      <h1 class="text-[0.6875rem] font-semibold uppercase tracking-[0.16em] text-muted-foreground">
        {t('title')}
      </h1>
      <select
        class={field}
        value={daemonI18n.lang}
        onchange={e => setDaemonLang(e.currentTarget.value)}
        aria-label="Language"
      >
        <option value="fr">FR</option>
        <option value="en">EN</option>
      </select>
    </div>

    {#if isDesktop}
      <DaemonDesktopControl onready={value => (desktopReady = value)} />
    {/if}

    {#if daemonError && (!isDesktop || desktopReady)}
      <div class="border border-stat-danger/40 bg-stat-danger/10 px-2.5 py-1.5 text-xs">
        <span class="font-medium text-stat-danger">{t('conn_err')} : {daemonError}</span>
        <span class="text-muted-foreground"> — {t('unavailable_hint')}</span>
      </div>
    {/if}

    {#if !isDesktop || desktopReady}
      <DaemonControl bind:error={daemonError} />
      <DaemonWatcher />
      <DaemonRules />
      <DaemonLogs />
    {/if}
  </div>

  {#if feedback.notice}
    <div
      role="status"
      class={cn(
        'fixed bottom-4 right-4 z-[90] max-w-sm border bg-card px-3 py-2 text-xs shadow-lg',
        noticeTones[feedback.notice.kind] ?? noticeTones.info
      )}
    >
      {feedback.notice.message}
    </div>
  {/if}

  <ConfirmDialog
    open={feedback.confirm.open}
    title={feedback.confirm.title}
    message={feedback.confirm.message}
    confirmLabel={feedback.confirm.confirmLabel}
    cancelLabel={feedback.confirm.cancelLabel}
    kind={feedback.confirm.kind}
    titleId="daemon-confirm-title"
    onCancel={() => resolveConfirm(false)}
    onConfirm={() => resolveConfirm(true)}
  />
{/if}
