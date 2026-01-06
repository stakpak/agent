use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CreditSchema {
    pub credit_amount: Option<f64>,
    pub credit_cost: Option<f64>,
    pub feature_id: Option<String>,
    pub metered_feature_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Feature {
    pub balance: Option<f64>,
    pub credit_schema: Option<Vec<CreditSchema>>,
    pub id: String,
    pub included_usage: Option<f64>,
    pub interval: Option<String>,
    pub interval_count: Option<u32>,
    pub name: Option<String>,
    pub next_reset_at: Option<f64>,
    pub overage_allowed: Option<bool>,
    pub type_: Option<String>,
    pub unlimited: Option<bool>,
    pub usage: Option<f64>,
    // For product items
    pub archived: Option<bool>,
    pub display: Option<HashMap<String, String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProductItem {
    pub feature: Feature,
    pub feature_id: String,
    pub feature_type: String,
    pub included_usage: Option<f64>,
    pub interval: Option<String>,
    pub interval_count: Option<u32>,
    pub reset_usage_when_enabled: Option<bool>,
    pub type_: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Product {
    pub id: String,
    pub is_add_on: bool,
    pub is_default: bool,
    pub items: Vec<ProductItem>,
    pub name: String,
    pub quantity: u32,
    pub started_at: u64,
    pub status: String,
    pub version: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BillingResponse {
    pub created_at: u64,
    pub email: String,
    pub env: String,
    pub features: HashMap<String, Feature>,
    pub id: String,
    pub name: String,
    pub products: Vec<Product>,
    pub stripe_id: Option<String>,
}
