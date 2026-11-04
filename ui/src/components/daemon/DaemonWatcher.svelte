<script>
  import { onMount } from 'svelte';
  import { daemonRequest } from '$lib/daemon/daemonApi.js';
  import { notify, ask } from '$lib/daemon/feedback.svelte.js';
  import { formatTimeLeft } from '$lib/daemon/format.js';
  import { t } from '$lib/daemon/i18n.svelte.js';
  import { btnDanger, btnRestart, btnStart, badgeBase, badgeTones } from '$lib/daemon/ui.js';
  import DaemonSection from './DaemonSection.svelte';

  const REFRESH_MS = 5000;

  let open = $state(false);
  let running = $state(null); // null = unknown / unreachable
  let crashError = $state('');
  let crashed = $state(false);
  let entries = $state([]); // [[tag, { errors, total_strikes, status, time_left }]]
  let inFlight = false;

  async function refresh() {
    if (inFlight) return;
    inFlight = true;
    try {
      const status = await daemonRequest('/watcher/status');
      if (!status.ok) throw new Error(`HTTP ${status.status}`);
      running = !!status.data?.running;
      crashed = status.data?.status === 'crashed';
      crashError = status.data?.error || '';

      const state = await daemonRequest('/watcher/state');
      if (state.ok) entries = Object.entries(state.data?.state ?? {});
    } catch {
      running = null;
    } finally {
      inFlight = false;
    }
  }

  async function watcherAction(action) {
    try {
      const { ok, data } = await daemonRequest(`/watcher/${action}`, { method: 'POST' });
      if (!ok || data?.success === false) {
        notify(`${t('action_err')}${data?.error ? ` (${data.error})` : ''}`, 'error');
      }
    } catch (e) {
      notify(`${t('action_err')} (${e?.message})`, 'error');
    }
    setTimeout(refresh, 1000);
  }

  async function clearAndResume(tag) {
    const confirmed = await ask({
      title: t('clearnresume'),
      message: `${t('clearnresume_msg')} : ${tag} ?`,
      confirmLabel: t('confirm'),
      cancelLabel: t('cancel'),
    });
    if (!confirmed) return;

    try {
      const { ok } = await daemonRequest('/watcher/clear_resume', {
        method: 'POST',
        body: { tag },
      });
      if (ok) setTimeout(refresh, 1000);
      else notify(t('clearnresume_err'), 'error');
    } catch (e) {
      notify(`${t('clearnresume_err')} (${e?.message})`, 'error');
    }
  }

  function statusTone(status) {
    if (status === 'paused') return badgeTones.paused;
    if (status === 'announce') return badgeTones.announce;
    return badgeTones.running;
  }

  function statusLabel(status) {
    if (status === 'paused') return t('paused');
    if (status === 'announce') return t('announce');
    return t('obs');
  }

  onMount(() => {
    refresh();
    const timer = setInterval(() => {
      if (document.visibilityState === 'visible') refresh();
    }, REFRESH_MS);
    return () => clearInterval(timer);
  });
</script>

<DaemonSection title={t('watcher_title')} bind:open>
  {#snippet actions()}
    <div class="flex flex-wrap items-center gap-1.5">
      {#if running === true}
        <span class="{badgeBase} {badgeTones.running}">{t('daemon_running')}</span>
      {:else if crashed}
        <span class="{badgeBase} {badgeTones.stopped}">{t('crash')} {crashError}</span>
      {:else if running === false}
        <span class="{badgeBase} {badgeTones.stopped}">{t('thread_inactive')}</span>
      {:else}
        <span class="{badgeBase} {badgeTones.neutral}">{t('conn_err')}</span>
      {/if}

      <button class={btnStart} disabled={running === true} onclick={() => watcherAction('start')}>
        {t('btn_start')}
      </button>
      <button class={btnDanger} disabled={running !== true} onclick={() => watcherAction('stop')}>
        {t('btn_stop')}
      </button>
      <button
        class={btnRestart}
        disabled={running !== true}
        onclick={() => watcherAction('restart')}
      >
        {t('btn_restart')}
      </button>
    </div>
  {/snippet}

  <div class="overflow-x-auto border border-border bg-background">
    <table class="w-full text-left text-xs">
      <thead>
        <tr class="border-b border-border text-xs uppercase tracking-wider text-muted-foreground">
          <th class="px-2.5 py-1.5 font-medium">{t('th_torrent')}</th>
          <th class="px-2.5 py-1.5 font-medium">{t('th_status')}</th>
          <th class="px-2.5 py-1.5 font-medium">{t('th_details')}</th>
          <th class="px-2.5 py-1.5 font-medium">{t('th_time')}</th>
          <th class="px-2.5 py-1.5"></th>
        </tr>
      </thead>
      <tbody>
        {#if entries.length === 0}
          <tr>
            <td colspan="5" class="px-2.5 py-4 text-center text-muted-foreground">
              {t('watcher_empty')}
            </td>
          </tr>
        {/if}

        {#each entries as [tag, info] (tag)}
          <tr class="border-b border-border align-top last:border-b-0 hover:bg-muted/30">
            <td class="max-w-[16rem] break-all px-2.5 py-1.5 font-medium text-foreground">{tag}</td>
            <td class="px-2.5 py-1.5">
              <span class="{badgeBase} {statusTone(info.status)}">{statusLabel(info.status)}</span>
            </td>
            <td class="px-2.5 py-1.5">
              <span class="font-semibold text-stat-ratio">
                {t('strikes')}
                {info.total_strikes}
              </span>
              {#each Object.entries(info.errors ?? {}) as [message, count] (message)}
                <div class="mt-0.5 text-muted-foreground">
                  - {message}: <strong class="text-foreground">{count}</strong>
                </div>
              {/each}
            </td>
            <td class="px-2.5 py-1.5 font-semibold tabular-nums text-stat-leecher">
              {formatTimeLeft(info.time_left, t('expiring'))}
            </td>
            <td class="px-2.5 py-1.5">
              <button class={btnStart} onclick={() => clearAndResume(tag)}>
                {t('clearnresume')}
              </button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
</DaemonSection>
