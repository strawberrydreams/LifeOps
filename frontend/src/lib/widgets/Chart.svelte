<script lang="ts">
  import type { ChartPoint, ChartSeries } from "../api";

  let {
    series = [],
    chartType = "line",
  }: {
    series: ChartSeries[];
    chartType?: "line" | "bar";
  } = $props();

  const width = 320;
  const height = 160;
  const pad = 18;
  const colors = ["#2563eb", "#16a34a", "#dc2626", "#9333ea", "#ea580c"];

  const usableWidth = width - pad * 2;
  const usableHeight = height - pad * 2;

  const allPoints = $derived(series.flatMap((s) => s.points).filter((p) => Number.isFinite(p.y)));
  const yValues = $derived(allPoints.map((p) => p.y));
  const minY = $derived(yValues.length ? Math.min(...yValues) : 0);
  const maxY = $derived(yValues.length ? Math.max(...yValues) : 1);
  const ySpan = $derived(maxY === minY ? 1 : maxY - minY);
  const xBuckets = $derived(sortedBuckets(allPoints));
  const bucketCount = $derived(Math.max(1, xBuckets.length));

  function compareBucketValue(left: unknown, right: unknown): number {
    if (typeof left === "number" && typeof right === "number" && Number.isFinite(left) && Number.isFinite(right)) {
      return left - right;
    }
    return String(left).localeCompare(String(right));
  }

  function sortedBuckets(points: ChartPoint[]): string[] {
    const buckets = new Map<string, unknown>();
    for (const point of points) {
      const key = String(point.x);
      if (!buckets.has(key)) buckets.set(key, point.x);
    }
    return Array.from(buckets.entries())
      .sort(([, left], [, right]) => compareBucketValue(left, right))
      .map(([key]) => key);
  }

  function bucketIndex(point: ChartPoint): number {
    const index = xBuckets.indexOf(String(point.x));
    return index >= 0 ? index : 0;
  }

  function xForBucket(index: number): number {
    if (bucketCount <= 1) return pad + usableWidth / 2;
    return pad + (index / (bucketCount - 1)) * usableWidth;
  }

  function yFor(point: ChartPoint): number {
    return pad + usableHeight - ((point.y - minY) / ySpan) * usableHeight;
  }

  function pathFor(points: ChartPoint[]): string {
    const finitePoints = points.filter((p) => Number.isFinite(p.y));
    return finitePoints
      .map((point, index) => {
        const command = index === 0 ? "M" : "L";
        return `${command} ${xForBucket(bucketIndex(point))} ${yFor(point)}`;
      })
      .join(" ");
  }

  function finitePoints(points: ChartPoint[]): ChartPoint[] {
    return points.filter((point) => Number.isFinite(point.y));
  }

  const bars = $derived(
    series.flatMap((item, seriesIndex) =>
      item.points
        .filter((point) => Number.isFinite(point.y))
        .map((point) => ({ point, bucketIndex: bucketIndex(point), seriesIndex }))
    )
  );

  function barX(bucketIndex: number, seriesIndex: number): number {
    const groupWidth = usableWidth / bucketCount;
    const seriesCount = Math.max(1, series.length);
    const barWidth = Math.max(2, groupWidth / seriesCount - 2);
    return pad + bucketIndex * groupWidth + seriesIndex * (barWidth + 2);
  }

  function barWidth(): number {
    const groupWidth = usableWidth / bucketCount;
    return Math.max(2, groupWidth / Math.max(1, series.length) - 2);
  }
</script>

<figure class="chart" style="margin: 0;">
  <svg viewBox={`0 0 ${width} ${height}`} role="img" aria-label="chart" style="display: block; width: 100%; max-width: 420px; height: auto;">
    <line class="axis" x1={pad} y1={height - pad} x2={width - pad} y2={height - pad} stroke="#d1d5db" stroke-width="1" />
    <line class="axis" x1={pad} y1={pad} x2={pad} y2={height - pad} stroke="#d1d5db" stroke-width="1" />

    {#if chartType === "bar"}
      {#each bars as bar}
        {@const y = yFor(bar.point)}
        <rect
          class="bar"
          x={barX(bar.bucketIndex, bar.seriesIndex)}
          y={y}
          width={barWidth()}
          height={Math.max(0, height - pad - y)}
          rx="2"
          fill={colors[bar.seriesIndex % colors.length]}
        />
      {/each}
    {:else}
      {#each series as item, index}
        <path
          class="series"
          d={pathFor(item.points)}
          fill="none"
          stroke={colors[index % colors.length]}
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
        {#each finitePoints(item.points) as point}
          <circle
            class="point"
            cx={xForBucket(bucketIndex(point))}
            cy={yFor(point)}
            r="3"
            fill={colors[index % colors.length]}
          />
        {/each}
      {/each}
    {/if}
  </svg>

  <figcaption class="legend" style="display: flex; flex-wrap: wrap; gap: 0.5rem 0.75rem; margin-top: 0.5rem; color: #374151; font-size: 0.875rem;">
    {#each series as item, index}
      <span class="legend-item" style="align-items: center; display: inline-flex; gap: 0.35rem;">
        <span class="swatch" style={`background: ${colors[index % colors.length]}; border-radius: 999px; display: inline-block; height: 0.6rem; width: 0.6rem;`}></span>
        {item.name}
      </span>
    {/each}
  </figcaption>
</figure>
