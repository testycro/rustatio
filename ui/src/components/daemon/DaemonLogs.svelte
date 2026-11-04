<script>
  import { onMount, tick } from 'svelte';
  import { daemonJson, daemonRequest } from '$lib/daemon/daemonApi.js';
  import { notify, ask } from '$lib/daemon/feedback.svelte.js';
  import { t } from '$lib/daemon/i18n.svelte.js';
  import { parseLogLines } from '$lib/daemon/logLines.js';
  import { btn, btnDanger, field } from '$lib/daemon/ui.js';
  import DaemonSection from './DaemonSection.svelte';

  const REFRESH_MS = 5000;

  let open = $state(true);
  let content = $state('');
  let lines = $state(100);
  let logEl = $state(null);
  let parsed = $derived(parseLogLines(content));
  let inFlight = false;

  async function fetchLogs() {
    if (inFlight) return;
    inFlight = true;
    try {
      const data = await daemonJson(`/logs?lines=${lines}`);
      const next = data.content ?? '';
      if (next === content) return;

      // Keep the reader's position: stick to the bottom only if already there.
      const el = logEl;
      const atBottom = !el || el.scrollHeight - el.clientHeight - el.scrollTop <= 30;
      const offsetFromBottom = el ? el.scrollHeight - el.scrollTop : 0;

      content = next;
      await tick();
      if (el) el.scrollTop = atBottom ? el.scrollHeight : el.scrollHeight - offsetFromBottom;
    } catch {
      // daemon unreachable: the control section already reports it
    } finally {
      inFlight = false;
    }
  }

  async function clearLogs() {
    const confirmed = await ask({
      title: t('btn_clear'),
      message: t('confirm_clear'),
      confirmLabel: t('confirm'),
      cancelLabel: t('cancel'),
      kind: 'danger',
    });
    if (!confirmed) return;

    try {
      const { ok } = await daemonRequest('/logs/clear', { method: 'POST' });
      if (ok) fetchLogs();
      else notify(t('err_clear'), 'error');
    } catch (e) {
      notify(`${t('err_clear')} (${e?.message})`, 'error');
    }
  }

  async function archiveLogs() {
    try {
      const { ok, data } = await daemonRequest('/logs/archive', { method: 'POST' });
      if (ok && data?.success) {
        notify(t('archive_success') + data.filename);
        fetchLogs();
      } else {
        notify(t('archive_err') + (data?.error || ''), 'error');
      }
    } catch (e) {
      notify(t('archive_err') + (e?.message || ''), 'error');
    }
  }

  onMount(() => {
    fetchLogs();
    const timer = setInterval(() => {
      if (document.visibilityState === 'visible') fetchLogs();
    }, REFRESH_MS);
    return () => clearInterval(timer);
  });
</script>

<DaemonSection title={t('logs_title')} bind:open>
  {#snippet actions()}
    <div class="flex flex-wrap items-center gap-1.5">
      <button class={btnDanger} onclick={clearLogs}>{t('btn_clear')}</button>
      <button class={btn} onclick={archiveLogs}>{t('btn_archive')}</button>
      <button class={btn} onclick={fetchLogs}>{t('btn_refresh')}</button>
      <label class="flex items-center gap-1 text-xs text-muted-foreground">
        {t('lbl_lines')}
        <select
          value={lines}
          onchange={e => {
            lines = Number(e.currentTarget.value);
            fetchLogs();
          }}
          class={field}
        >
          {#each [50, 100, 200, 500, 1000] as n (n)}
            <option value={n}>{n}</option>
          {/each}
        </select>
      </label>
    </div>
  {/snippet}

  <div
    bind:this={logEl}
    class="h-[28rem] min-h-40 resize-y overflow-auto border border-border bg-background py-1 font-mono text-xs leading-relaxed"
  >
    {#each parsed as line (line.id)}
      <div class="dm-log dm-log-{line.tone} whitespace-pre-wrap break-all">
        {#if line.time}<span class="dm-log-time" title={line.time}>{line.time.slice(11)}</span
          >{/if}{line.body}
      </div>
    {/each}
  </div>
</DaemonSection>
