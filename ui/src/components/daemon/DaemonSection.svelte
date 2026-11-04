<script>
  import { ChevronRight } from '@lucide/svelte';
  import { cn } from '$lib/core/utils.js';

  let { title, open = $bindable(true), actions, children } = $props();
</script>

<section class="border border-border bg-card">
  <div
    class={cn(
      'flex min-h-9 flex-wrap items-center justify-between gap-2 px-2.5 py-1',
      open && 'border-b border-border'
    )}
  >
    <button
      type="button"
      class="flex items-center gap-1.5 text-muted-foreground transition-colors hover:text-primary cursor-pointer"
      onclick={() => (open = !open)}
      aria-expanded={open}
    >
      <ChevronRight size={12} class={cn('transition-transform', open && 'rotate-90')} />
      <span class="text-xs font-semibold uppercase tracking-[0.14em] text-foreground">{title}</span>
    </button>
    {@render actions?.()}
  </div>

  {#if open}
    <div class="p-2.5">
      {@render children?.()}
    </div>
  {/if}
</section>
