#[allow(dead_code)]
pub struct ModelPrice {
    pub name: &'static str,
    pub input_per_mtok: f64,   // USD per million input tokens
    pub output_per_mtok: f64,  // USD per million output tokens
}

pub const MODEL_PRICES: &[ModelPrice] = &[
    ModelPrice { name: "claude-sonnet-4", input_per_mtok: 3.0, output_per_mtok: 15.0 },
    ModelPrice { name: "claude-haiku-4", input_per_mtok: 0.80, output_per_mtok: 4.0 },
    ModelPrice { name: "gpt-4o", input_per_mtok: 2.50, output_per_mtok: 10.0 },
    ModelPrice { name: "gpt-4o-mini", input_per_mtok: 0.15, output_per_mtok: 0.60 },
    ModelPrice { name: "gemini-2.5-flash", input_per_mtok: 0.15, output_per_mtok: 0.60 },
    ModelPrice { name: "gemini-2.5-pro", input_per_mtok: 1.25, output_per_mtok: 10.0 },
    // Default fallback
    ModelPrice { name: "default", input_per_mtok: 3.0, output_per_mtok: 15.0 },
];

pub fn get_price(model: &str) -> &'static ModelPrice {
    let model_lower = model.to_lowercase();
    for price in MODEL_PRICES {
        if price.name == model_lower {
            return price;
        }
    }
    for price in MODEL_PRICES {
        if price.name != "default" && (model_lower.contains(price.name) || price.name.contains(&model_lower)) {
            return price;
        }
    }
    &MODEL_PRICES[MODEL_PRICES.len() - 1]
}

pub fn estimate_cost_usd(model: &str, tokens_saved: u64) -> f64 {
    let price = get_price(model);
    (tokens_saved as f64) / 1_000_000.0 * price.input_per_mtok
}
