<script>
  import { onMount } from 'svelte';
  import { ArrowRight, Plus, Trash2, X } from '@lucide/svelte';
  import { cn } from '$lib/core/utils.js';
  import { daemonJson, daemonRequest } from '$lib/daemon/daemonApi.js';
  import { notify } from '$lib/daemon/feedback.svelte.js';
  import { t } from '$lib/daemon/i18n.svelte.js';
  import {
    ACTIONS,
    CONFIG_KEYS,
    DEFAULT_CONFIG_SUGGESTIONS,
    FIELDS,
    LOGICAL_OPS,
    OPERATORS,
    createCondition,
    createRule,
    isSimpleAction,
    parseRules,
    serializeRules,
    withCurrent,
  } from '$lib/daemon/rules.js';
  import { btn, btnPrimary, field } from '$lib/daemon/ui.js';
  import DaemonSection from './DaemonSection.svelte';

  let open = $state(false);
  let rules = $state([createRule()]);
  let rawText = $state('');
  let rawMode = $state(false);
  let baseline = $state(''); // serialized visual rules right after a load / parse
  let loading = $state(true);
  let loadError = $state('');
  let saving = $state(false);

  // True only when the visual editor was really changed. Saving an untouched file
  // keeps it byte for byte (comments and blank lines included).
  let visualDirty = $derived(serializeRules(rules) !== baseline);

  function resetVisual(text) {
    rules = parseRules(text);
    baseline = serializeRules(rules);
  }

  onMount(async () => {
    try {
      const data = await daemonJson('/rules');
      rawText = data.content || '';
      resetVisual(rawText);
    } catch (e) {
      loadError = e?.message || 'error';
    } finally {
      loading = false;
    }
  });

  function toggleRaw() {
    if (rawMode) {
      resetVisual(rawText);
    } else if (visualDirty) {
      rawText = serializeRules(rules);
    }
    rawMode = !rawMode;
  }

  function addRule() {
    rules.push(createRule());
  }

  function removeRule(id) {
    rules = rules.filter(rule => rule.id !== id);
  }

  function addCondition(rule) {
    rule.conditions.push(createCondition('AND'));
  }

  function removeCondition(rule, id) {
    rule.conditions = rule.conditions.filter(condition => condition.id !== id);
  }

  async function save() {
    const content = rawMode || !visualDirty ? rawText : serializeRules(rules);

    saving = true;
    try {
      const { ok, data } = await daemonRequest('/rules', { method: 'POST', body: { content } });
      if (ok && data?.success !== false) {
        rawText = content;
        if (!rawMode) baseline = serializeRules(rules);
        notify(t('rule_saved'));
      } else {
        notify(`${t('rule_save_err')}${data?.error ? ` (${data.error})` : ''}`, 'error');
      }
    } catch (e) {
      notify(`${t('rule_save_err')} (${e?.message})`, 'error');
    } finally {
      saving = false;
    }
  }

  const blockTitle =
    'text-[0.6875rem] font-semibold uppercase tracking-wider text-muted-foreground';
</script>

<datalist id="daemon-default-config-list">
  {#each DEFAULT_CONFIG_SUGGESTIONS as suggestion (suggestion)}
    <option value={suggestion}></option>
  {/each}
</datalist>

<DaemonSection title={t('rules_title')} bind:open>
  {#if loading}
    <div class="text-xs text-muted-foreground">…</div>
  {:else if loadError}
    <div class="text-xs text-stat-danger">{loadError}</div>
  {:else}
    <p class="mb-2 text-xs text-muted-foreground">{t('rules_desc')}</p>

    {#if rawMode}
      <p class="mb-1 text-xs text-primary">{t('raw_hint')}</p>
      <textarea
        bind:value={rawText}
        spellcheck="false"
        rows="14"
        class="{field} h-auto w-full resize-y py-1.5 font-mono leading-relaxed"
      ></textarea>
    {:else}
      <div class="space-y-2">
        {#each rules as rule (rule.id)}
          {@const simple = isSimpleAction(rule.action)}
          <div
            class="dm-rule dm-act-{rule.action} flex flex-wrap items-stretch gap-2 border border-border bg-background p-2"
          >
            <!-- 1. Conditions -->
            <div class="flex min-w-[18rem] flex-[2] flex-col gap-1">
              <span class={blockTitle}>{t('rule_cond')}</span>
              {#each rule.conditions as condition, index (condition.id)}
                <div class="flex flex-wrap items-center gap-1">
                  {#if index > 0}
                    <select bind:value={condition.logOp} class={field}>
                      {#each LOGICAL_OPS as op (op)}
                        <option value={op}>{op}</option>
                      {/each}
                    </select>
                  {/if}
                  <select bind:value={condition.field} class="{field} min-w-0 flex-1">
                    {#each withCurrent(FIELDS, condition.field) as name (name)}
                      <option value={name}>{name}</option>
                    {/each}
                  </select>
                  <select bind:value={condition.op} class={field}>
                    {#each OPERATORS as op (op)}
                      <option value={op}>{op}</option>
                    {/each}
                  </select>
                  <input
                    type="text"
                    bind:value={condition.val}
                    placeholder={condition.op === ':' ? t('ph_ex') : t('ph_val_only')}
                    class="{field} w-40"
                  />
                  {#if index > 0}
                    <button
                      type="button"
                      class="p-1 text-muted-foreground transition-colors hover:text-stat-danger cursor-pointer"
                      onclick={() => removeCondition(rule, condition.id)}
                      title={t('btn_del')}
                      aria-label={t('btn_del')}
                    >
                      <X size={12} />
                    </button>
                  {/if}
                </div>
              {/each}
              <button type="button" class="{btn} self-start" onclick={() => addCondition(rule)}>
                <Plus size={11} />
                {t('btn_add_cond')}
              </button>
            </div>

            <ArrowRight size={14} class="dm-arrow hidden flex-shrink-0 self-center lg:block" />

            <!-- 2. Action -->
            <div class="flex min-w-[9rem] flex-col gap-1">
              <span class={blockTitle}>{t('rule_act')}</span>
              <select
                bind:value={rule.action}
                class="{field} dm-tone dm-act-{rule.action} font-semibold uppercase"
              >
                {#each withCurrent(ACTIONS, rule.action) as action (action)}
                  <option value={action}>{action}</option>
                {/each}
              </select>
            </div>

            <ArrowRight
              size={14}
              class={cn(
                'dm-arrow hidden flex-shrink-0 self-center lg:block',
                simple && 'opacity-40'
              )}
            />

            <!-- 3. Parameters -->
            <div class={cn('flex min-w-[16rem] flex-[2] flex-col gap-1', simple && 'opacity-50')}>
              <span class={blockTitle}>{t('rule_param')}</span>
              {#if simple}
                <span class="text-xs italic text-muted-foreground">{t('no_params')}</span>
              {:else if rule.action === 'update'}
                <div class="flex items-center gap-1.5">
                  <select bind:value={rule.conf} class="{field} min-w-0 flex-1">
                    {#each withCurrent(CONFIG_KEYS, rule.conf) as key (key)}
                      <option value={key}>{key}</option>
                    {/each}
                  </select>
                  <span class="font-bold text-muted-foreground">=</span>
                  <input
                    type="text"
                    list="daemon-default-config-list"
                    bind:value={rule.value}
                    placeholder={t('ph_val')}
                    class="{field} w-44"
                  />
                </div>
              {:else}
                <input
                  type="text"
                  bind:value={rule.text}
                  placeholder={t('ph_param')}
                  class="{field} w-full"
                />
              {/if}
            </div>

            <button
              type="button"
              class="self-start p-1 text-muted-foreground transition-colors hover:text-stat-danger cursor-pointer"
              onclick={() => removeRule(rule.id)}
              title={t('btn_del_rule')}
              aria-label={t('btn_del_rule')}
            >
              <Trash2 size={13} />
            </button>
          </div>
        {/each}
      </div>
    {/if}

    <div class="mt-2 flex flex-wrap items-center gap-2">
      {#if !rawMode}
        <button type="button" class={btn} onclick={addRule}>
          <Plus size={11} />
          {t('btn_add_rule')}
        </button>
      {/if}
      <button type="button" class={btn} onclick={toggleRaw}>
        {rawMode ? t('btn_visual_view') : t('btn_raw_view')}
      </button>
      <button type="button" class="{btnPrimary} ml-auto" disabled={saving} onclick={save}>
        {t('btn_save_rules')}
      </button>
    </div>
  {/if}
</DaemonSection>
