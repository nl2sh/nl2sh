import type {ChartSpec} from './chartData';

const colors = ['var(--chart-1)', 'var(--chart-2)', 'var(--chart-3)', 'var(--chart-4)', 'var(--chart-5)', 'var(--chart-6)'];

function formatted(value: number, unit: string): string {
  return `${new Intl.NumberFormat(undefined, {maximumFractionDigits: 3}).format(value)}${unit ? ` ${unit}` : ''}`;
}

function LinePlot({spec}: {spec: ChartSpec}) {
  const max = Math.max(...spec.values, 1);
  const width = 600, height = 200, left = 45, right = 15, top = 15, bottom = 35;
  const x = (index: number) => left + index * (width - left - right) / Math.max(1, spec.values.length - 1);
  const y = (value: number) => top + (height - top - bottom) * (1 - value / max);
  const points = spec.values.map((value, index) => `${x(index)},${y(value)}`).join(' ');
  const stride = Math.max(1, Math.ceil(spec.labels.length / 7));
  return <svg class="chart-line" viewBox={`0 0 ${width} ${height}`} role="img" aria-label={`${spec.title} 折线图`}>
    {[0, .5, 1].map(fraction => <g key={fraction}><line x1={left} x2={width-right} y1={y(max*fraction)} y2={y(max*fraction)} class="chart-grid"/><text x={left-7} y={y(max*fraction)+4} text-anchor="end">{new Intl.NumberFormat(undefined,{notation:'compact',maximumFractionDigits:1}).format(max*fraction)}</text></g>)}
    <polyline points={points} class="chart-stroke"/>
    {spec.values.map((value,index) => <circle key={index} cx={x(index)} cy={y(value)} r="4" class="chart-point"><title>{`${spec.labels[index]}: ${formatted(value,spec.unit)}`}</title></circle>)}
    {spec.labels.map((label,index) => index % stride === 0 || index === spec.labels.length-1 ? <text key={index} x={x(index)} y={height-8} text-anchor="middle">{label.length > 10 ? `${label.slice(0,9)}…` : label}</text> : null)}
  </svg>;
}

export function Chart({spec}: {spec: ChartSpec}) {
  const max = Math.max(...spec.values, 1);
  const total = spec.values.reduce((sum, value) => sum + value, 0);
  let position = 0;
  const sectors = spec.chart_type === 'pie' ? spec.values.map((value,index) => {
    const start = position;
    position += value / total * 100;
    return `${colors[index % colors.length]} ${start}% ${position}%`;
  }) : [];
  return <figure class="chart" aria-label={`${spec.title} ${spec.chart_type} chart`}>
    <figcaption><strong>{spec.title}</strong><small>来源（模型提供）：{spec.source}</small></figcaption>
    {spec.chart_type === 'bar' && <div class="chart-bars">{spec.labels.map((label,index) => <div class="chart-bar-row" key={index}><span>{label}</span><div class="chart-bar-track"><div style={{width:`${spec.values[index] / max * 100}%`,background:colors[index % colors.length]}}/></div><b>{formatted(spec.values[index],spec.unit)}</b></div>)}</div>}
    {spec.chart_type === 'line' && <LinePlot spec={spec}/>}
    {spec.chart_type === 'pie' && <div class="chart-pie-layout"><div class="chart-pie" role="img" aria-label="饼图" style={{background:`conic-gradient(${sectors.join(',')})`}}/><ul>{spec.labels.map((label,index) => <li key={index}><i style={{background:colors[index % colors.length]}}/>{label}：{formatted(spec.values[index],spec.unit)}（{(spec.values[index] / total * 100).toFixed(1)}%）</li>)}</ul></div>}
    <details class="chart-data"><summary>查看数据</summary><div><table><thead><tr><th>项目</th><th>数值</th></tr></thead><tbody>{spec.labels.map((label,index) => <tr key={index}><td>{label}</td><td>{formatted(spec.values[index],spec.unit)}</td></tr>)}</tbody></table></div></details>
  </figure>;
}
