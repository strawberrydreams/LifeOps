<script lang="ts">
  let {
    value,
    onchange,
    id,
    labelledby,
    describedby,
  }: {
    value: { amount: number; currency: string } | null;
    onchange: (v: { amount: number; currency: string } | null) => void;
    id?: string;
    labelledby?: string;
    describedby?: string;
  } = $props();

  const cur = $derived(value?.currency ?? "KRW");
  const amt = $derived(value?.amount ?? null);

  function setAmount(s: string) {
    if (s === "") {
      onchange(null);
      return;
    }
    onchange({ amount: Number(s), currency: cur });
  }

  function setCurrency(c: string) {
    onchange({ amount: amt ?? 0, currency: c });
  }
</script>

<span class="money" {id} role="group" aria-labelledby={labelledby} aria-describedby={describedby}>
  <input
    id={id ? `${id}-amount` : undefined}
    type="number"
    placeholder="금액"
    value={amt ?? ""}
    aria-label="금액"
    oninput={(e) => setAmount((e.currentTarget as HTMLInputElement).value)}
  />
  <input
    id={id ? `${id}-currency` : undefined}
    placeholder="통화"
    value={cur}
    aria-label="통화"
    oninput={(e) => setCurrency((e.currentTarget as HTMLInputElement).value)}
  />
</span>
