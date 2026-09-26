//! Bounded, presentation-only chart data shared by the Agent and interfaces.

use super::{
    PreparedExecution, PreparedToolCall, ToolCategory, ToolContext, ToolMetadata, ToolOutput,
    ToolRisk,
};
use anyhow::{bail, Result};
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const META: ToolMetadata = ToolMetadata {
    name: "create_chart",
    description: "Present existing numeric evidence as a bar, line, or pie chart. Copy values from user input or completed tool results; never invent or estimate values. Include a short source label. This tool only validates and displays data; it does not collect or verify statistics.",
    category: ToolCategory::Chart,
    risk: ToolRisk::ReadOnly,
    requires: &[],
    parallel_safe: true,
};

/// Data accepted and returned by the presentation-only chart tool.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChartSpec {
    /// Chart kind supported by both browser and text fallback.
    pub chart_type: ChartType,
    /// Short heading for the chart.
    pub title: String,
    /// Label describing where the numbers came from.
    pub source: String,
    /// Optional unit shown next to values.
    #[serde(default)]
    pub unit: String,
    /// Category labels in display order.
    pub labels: Vec<String>,
    /// One value for each label.
    pub values: Vec<f64>,
}

/// Supported presentation types.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChartType {
    /// Horizontal comparison bars.
    Bar,
    /// Ordered values joined by a line.
    Line,
    /// Parts of a nonzero whole.
    Pie,
}

impl ChartSpec {
    fn validate(&self) -> Result<()> {
        if !valid_text(&self.title, 120) {
            bail!("chart title must contain 1–120 bytes")
        }
        if !valid_text(&self.source, 200) {
            bail!("chart source must contain 1–200 bytes")
        }
        if self.unit.len() > 32 || self.unit.chars().any(char::is_control) {
            bail!("chart unit exceeds 32 bytes")
        }
        if self.labels.is_empty()
            || self.labels.len() > 32
            || self.labels.len() != self.values.len()
        {
            bail!("chart requires 1–32 matching labels and values")
        }
        if matches!(self.chart_type, ChartType::Pie) && self.labels.len() > 12 {
            bail!("pie chart supports at most 12 slices")
        }
        if self.labels.iter().any(|label| !valid_text(label, 80)) {
            bail!("chart labels must contain 1–80 bytes")
        }
        if self
            .values
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0 || *value > 1e12)
        {
            bail!("chart values must be finite numbers from 0 to 1e12")
        }
        if matches!(self.chart_type, ChartType::Pie) && self.values.iter().sum::<f64>() <= 0.0 {
            bail!("pie chart requires a positive total")
        }
        Ok(())
    }
}

fn valid_text(value: &str, max_bytes: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max_bytes && !value.chars().any(char::is_control)
}

struct ChartOperation(ChartSpec);

#[async_trait]
impl PreparedExecution for ChartOperation {
    async fn execute(self: Box<Self>, _: &mut ToolContext<'_>) -> Result<ToolOutput> {
        Ok(ToolOutput::success(serde_json::to_string(&self.0)?))
    }
}

async fn prepare_chart(_: &ToolContext<'_>, spec: ChartSpec) -> Result<PreparedToolCall> {
    spec.validate()?;
    Ok(PreparedToolCall::operation(
        String::new(),
        Box::new(ChartOperation(spec)),
    ))
}

define_tool!(ChartTool, ChartSpec, META, prepare_chart);

/// Converts a validated chart result to a readable terminal fallback.
pub fn text_fallback(output: &str) -> Option<String> {
    let spec: ChartSpec = serde_json::from_str(output).ok()?;
    spec.validate().ok()?;
    let mut lines = vec![spec.title, format!("来源（模型提供）: {}", spec.source)];
    for (label, value) in spec.labels.iter().zip(&spec.values) {
        lines.push(format!(
            "{label}: {value}{}",
            if spec.unit.is_empty() {
                String::new()
            } else {
                format!(" {}", spec.unit)
            }
        ));
    }
    Some(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chart_data_is_bounded_and_has_text_fallback() {
        let spec = ChartSpec {
            chart_type: ChartType::Bar,
            title: "存储".into(),
            source: "android_storage result".into(),
            unit: "GiB".into(),
            labels: vec!["应用".into()],
            values: vec![2.5],
        };
        assert!(spec.validate().is_ok());
        let encoded = serde_json::to_string(&spec).unwrap_or_default();
        assert!(text_fallback(&encoded).is_some_and(|text| text.contains("应用: 2.5 GiB")));
        let mut invalid = spec;
        invalid.values = vec![f64::NAN];
        assert!(invalid.validate().is_err());
        invalid.values = vec![1.0, 2.0];
        assert!(invalid.validate().is_err());
        invalid.values = vec![1.0];
        invalid.title = "\u{1b}[2J".into();
        assert!(invalid.validate().is_err());
    }
}
