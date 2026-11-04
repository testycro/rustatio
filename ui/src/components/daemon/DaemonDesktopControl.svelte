<script>
  import { onMount } from 'svelte';
  import { notify } from '$lib/daemon/feedback.svelte.js';
  import { t } from '$lib/daemon/i18n.svelte.js';
  import { badgeBase, badgeTones, btnDanger, btnStart } from '$lib/daemon/ui.js';
  import DaemonSection from './DaemonSection.svelte';

  // Desktop app only: the daemon is a process of the app, started with the app only if the user
  // turned the autostart on, and always stopped with it. The parent hides the other sections
  // while the daemon does not answer: it is told through `onready`.
  let { onready } = $props();

  let open = $state(true);
  let status = $state(null);
  let busy = $state(false);
  let inFlight = false;

  async function call(command, args) {
    const { invoke } = await import('@tauri-apps/api/core');
    return invoke(command, args);
  }

  async function refresh() {
    if (inFlight) return;
    inFlight = true;
    try {
      status = await call('daemon_status');
      onready?.(status.ready);
    } catch {
      onready?.(false);
    } finally {
      inFlight = false;
    }
  }

  async function act(command, args) {
    busy = true;
    try {
      await call(command, args);
    } catch (e) {
      notify(`${t('action_err')} (${String(e)})`, 'error');
    } finally {
      busy = false;
      await refresh();
    }
  }

  async function onAutostartChange(event) {
    await act('daemon_set_autostart', { enabled: event.currentTarget.checked });
  }

  onMount(() => {
    refresh();
    const timer = setInterval(refresh, 2000);
    return () => clearInterval(timer);
  });
</script>

{#if status && !status.available}
  <div class="border border-border bg-card px-2.5 py-2 text-xs text-muted-foreground">
    {t('desk_unavailable')}
  </div>
{:else}
  <DaemonSection title={t('desk_title')} bind:open>
    {#snippet actions()}
      <div class="flex flex-wrap items-center gap-1.5">
        {#if status?.ready}
          <span class="{badgeBase} {badgeTones.running}">{t('desk_running')}</span>
        {:else if status?.wanted}
          <span class="{badgeBase} {badgeTones.paused}">{t('desk_starting')}</span>
        {:else}
          <span class="{badgeBase} {badgeTones.stopped}">{t('desk_stopped')}</span>
        {/if}

        <button
          class={btnStart}
          disabled={busy || !status || status.wanted}
          onclick={() => act('daemon_start')}
        >
          {t('desk_start')}
        </button>
        <button
          class={btnDanger}
          disabled={busy || !status || !status.wanted}
          onclick={() => act('daemon_stop')}
        >
          {t('desk_stop')}
        </button>
      </div>
    {/snippet}

    <label
      class="flex items-start gap-2 text-[0.6875rem] leading-tight text-muted-foreground cursor-pointer"
    >
      <input
        type="checkbox"
        class="mt-0.5"
        checked={status?.autostart ?? false}
        onchange={onAutostartChange}
        disabled={busy || !status}
      />
      <span>
        {t('desk_autostart')}
        <span class="block text-[0.6875rem] text-muted-foreground/80">
          {t('desk_autostart_hint')}
        </span>
      </span>
    </label>

    {#if status && !status.ready && !status.wanted}
      <p class="mt-2 text-xs text-muted-foreground">{t('desk_stopped_hint')}</p>
    {/if}
  </DaemonSection>
{/if}
