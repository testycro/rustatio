<script>
  import { onMount } from 'svelte';
  import { daemonRequest } from '$lib/daemon/daemonApi.js';
  import { notify, ask } from '$lib/daemon/feedback.svelte.js';
  import { t } from '$lib/daemon/i18n.svelte.js';
  import { btn, btnDanger, btnRestart, btnStart, badgeBase, badgeTones } from '$lib/daemon/ui.js';
  import DaemonSection from './DaemonSection.svelte';
  import DaemonEnvConfig from './DaemonEnvConfig.svelte';

  // `error` is bound by the parent to show a "daemon unreachable" banner.
  let { error = $bindable('') } = $props();

  let open = $state(false);
  let running = $state(null); // null = unknown
  let pid = $state(null);
  let inFlight = false;

  async function refresh() {
    if (inFlight) return;
    inFlight = true;
    try {
      const { ok, status, data } = await daemonRequest('/status');
      if (!ok) throw new Error(data?.error || `HTTP ${status}`);
      running = !!data.running;
      pid = data.pid ?? null;
      error = '';
    } catch (e) {
      running = null;
      error = e?.message || 'error';
    } finally {
      inFlight = false;
    }
  }

  async function post(path, failedMessage) {
    try {
      const { ok, data } = await daemonRequest(path, { method: 'POST' });
      if (!ok || data?.success === false) {
        notify(`${failedMessage}${data?.error ? ` (${data.error})` : ''}`, 'error');
        return false;
      }
      return true;
    } catch (e) {
      notify(`${failedMessage} (${e?.message})`, 'error');
      return false;
    }
  }

  async function daemonAction(action) {
    await post(`/daemon/${action}`, t('action_err'));
    setTimeout(refresh, 1000);
  }

  async function restartScript() {
    const confirmed = await ask({
      title: t('btn_script_restart'),
      message: t('confirm_restart'),
      confirmLabel: t('confirm'),
      cancelLabel: t('cancel'),
    });
    if (!confirmed) return;

    if (await post('/admin/restart', t('err_restart'))) notify(t('restart_msg'), 'info');
  }

  onMount(() => {
    refresh();
    const timer = setInterval(() => {
      if (document.visibilityState === 'visible') refresh();
    }, 3000);
    return () => clearInterval(timer);
  });
</script>

<DaemonSection title={t('ctrl_title')} bind:open>
  {#snippet actions()}
    <div class="flex flex-wrap items-center gap-1.5">
      {#if running === true}
        <span class="{badgeBase} {badgeTones.running}">
          {t('daemon_running')}{pid ? ` (PID ${pid})` : ''}
        </span>
      {:else if running === false}
        <span class="{badgeBase} {badgeTones.stopped}">{t('daemon_stopped')}</span>
      {:else if error}
        <span class="{badgeBase} {badgeTones.stopped}">{t('conn_err')}</span>
      {:else}
        <span class="{badgeBase} {badgeTones.neutral}">{t('daemon_checking')}</span>
      {/if}

      <button class={btnStart} disabled={running === true} onclick={() => daemonAction('start')}>
        {t('btn_daemon_start')}
      </button>
      <button class={btnDanger} disabled={running !== true} onclick={() => daemonAction('stop')}>
        {t('btn_daemon_stop')}
      </button>
      <button
        class={btnRestart}
        disabled={running !== true}
        onclick={() => daemonAction('restart')}
      >
        {t('btn_daemon_restart')}
      </button>
      <button class={btn} onclick={restartScript}>{t('btn_script_restart')}</button>
    </div>
  {/snippet}

  <DaemonEnvConfig />
</DaemonSection>
