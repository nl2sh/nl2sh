import type {Entry} from './types';

export interface ChartSpec {
  chart_type: 'bar' | 'line' | 'pie';
  title: string;
  source: string;
  unit: string;
  labels: string[];
  values: number[];
}

export function parseChart(call: Entry | null, result: Entry | null): ChartSpec | null {
  if (call?.text !== 'create_chart' || result?.kind !== 'tool_result') return null;
  try {
    const data: unknown = JSON.parse(result.text);
    if (!data || typeof data !== 'object') return null;
    const spec = data as Partial<ChartSpec>;
    if (!['bar', 'line', 'pie'].includes(spec.chart_type || '') ||
        typeof spec.title !== 'string' || !spec.title.trim() || spec.title.length > 120 || /[\x00-\x1f\x7f]/.test(spec.title) ||
        typeof spec.source !== 'string' || !spec.source.trim() || spec.source.length > 200 || /[\x00-\x1f\x7f]/.test(spec.source) ||
        typeof spec.unit !== 'string' || spec.unit.length > 32 || /[\x00-\x1f\x7f]/.test(spec.unit) ||
        !Array.isArray(spec.labels) || !Array.isArray(spec.values) ||
        spec.labels.length < 1 || spec.labels.length > 32 || spec.labels.length !== spec.values.length ||
        (spec.chart_type === 'pie' && spec.labels.length > 12) ||
        !spec.labels.every(label => typeof label === 'string' && label.trim() && label.length <= 80 && !/[\x00-\x1f\x7f]/.test(label)) ||
        !spec.values.every(value => typeof value === 'number' && Number.isFinite(value) && value >= 0 && value <= 1e12) ||
        (spec.chart_type === 'pie' && spec.values.reduce((sum, value) => sum + value, 0) <= 0)) return null;
    return spec as ChartSpec;
  } catch {
    return null;
  }
}
