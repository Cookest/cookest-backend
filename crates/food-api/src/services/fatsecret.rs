use std::sync::Arc;
use tokio::sync::RwLock;
use chrono::{DateTime, Utc, Duration};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Deserializer};
use reqwest::Client;

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: i64,
}

pub struct FatSecretClient {
    client: Client,
    client_id: String,
    client_secret: SecretString,
    token_cache: RwLock<Option<(String, DateTime<Utc>)>>,
}

impl FatSecretClient {
    pub fn new(client_id: String, client_secret: SecretString) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .connect_timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_else(|_| Client::new());
        Self {
            client,
            client_id,
            client_secret,
            token_cache: RwLock::new(None),
        }
    }

    async fn send_with_retry<T, F, Fut>(&self, make_request: F) -> Result<T, reqwest::Error>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<reqwest::Response, reqwest::Error>>,
        T: serde::de::DeserializeOwned,
    {
        let mut attempts = 0;
        let max_attempts = 3;
        let mut delay = std::time::Duration::from_millis(500);

        loop {
            attempts += 1;
            match make_request().await {
                Ok(resp) => {
                    let status = resp.status();
                    if (status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS)
                        && attempts < max_attempts
                    {
                        tracing::warn!(
                            "FatSecret request failed with status {}, retrying (attempt {}/{}) after {:?}",
                            status,
                            attempts,
                            max_attempts,
                            delay
                        );
                        tokio::time::sleep(delay).await;
                        delay *= 2;
                        continue;
                    }
                    let resp = resp.error_for_status()?;
                    return resp.json::<T>().await;
                }
                Err(e) => {
                    let is_transient = !e.is_builder();
                    if is_transient && attempts < max_attempts {
                        tracing::warn!(
                            "FatSecret request error: {}, retrying (attempt {}/{}) after {:?}",
                            e,
                            attempts,
                            max_attempts,
                            delay
                        );
                        tokio::time::sleep(delay).await;
                        delay *= 2;
                        continue;
                    }
                    return Err(e);
                }
            }
        }
    }

    async fn get_token(&self) -> Result<String, reqwest::Error> {
        {
            let cache = self.token_cache.read().await;
            if let Some((token, expiry)) = &*cache {
                if Utc::now() + Duration::seconds(30) < *expiry {
                    return Ok(token.clone());
                }
            }
        }

        let mut cache = self.token_cache.write().await;
        if let Some((token, expiry)) = &*cache {
            if Utc::now() + Duration::seconds(30) < *expiry {
                return Ok(token.clone());
            }
        }

        tracing::info!("Requesting new FatSecret OAuth2 token");
        let resp: TokenResponse = self.send_with_retry(|| {
            self.client
                .post("https://oauth.fatsecret.com/connect/token")
                .basic_auth(&self.client_id, Some(self.client_secret.expose_secret()))
                .form(&[
                    ("grant_type", "client_credentials"),
                    ("scope", "basic"),
                ])
                .send()
        })
        .await?;

        let expiry = Utc::now() + Duration::seconds(resp.expires_in);
        *cache = Some((resp.access_token.clone(), expiry));
        Ok(resp.access_token)
    }

    pub async fn search_recipes(
        &self,
        query: Option<&str>,
        page_number: u64,
        max_results: u64,
    ) -> Result<FSRecipeSearchWrapper, reqwest::Error> {
        let token = self.get_token().await?;
        self.send_with_retry(|| {
            let mut req = self.client.get("https://platform.fatsecret.com/rest/server.api")
                .bearer_auth(&token)
                .query(&[
                    ("method", "recipes.search.v3"),
                    ("format", "json"),
                    ("page_number", &page_number.to_string()),
                    ("max_results", &max_results.to_string()),
                ]);

            if let Some(q) = query {
                req = req.query(&[("search_expression", q)]);
            }

            req.send()
        })
        .await
    }

    pub async fn get_recipe(&self, recipe_id: i64) -> Result<FSRecipeDetailWrapper, reqwest::Error> {
        let token = self.get_token().await?;
        self.send_with_retry(|| {
            self.client.get("https://platform.fatsecret.com/rest/server.api")
                .bearer_auth(&token)
                .query(&[
                    ("method", "recipe.get.v2"),
                    ("format", "json"),
                    ("recipe_id", &recipe_id.to_string()),
                ])
                .send()
        })
        .await
    }

    pub async fn search_ingredients(
        &self,
        query: Option<&str>,
        page_number: u64,
        max_results: u64,
    ) -> Result<FSFoodsSearchWrapper, reqwest::Error> {
        let token = self.get_token().await?;
        self.send_with_retry(|| {
            let mut req = self.client.get("https://platform.fatsecret.com/rest/server.api")
                .bearer_auth(&token)
                .query(&[
                    ("method", "foods.search"),
                    ("format", "json"),
                    ("page_number", &page_number.to_string()),
                    ("max_results", &max_results.to_string()),
                ]);

            if let Some(q) = query {
                req = req.query(&[("search_expression", q)]);
            }

            req.send()
        })
        .await
    }

    pub async fn get_ingredient(&self, food_id: i64) -> Result<FSFoodDetailWrapper, reqwest::Error> {
        let token = self.get_token().await?;
        self.send_with_retry(|| {
            self.client.get("https://platform.fatsecret.com/rest/server.api")
                .bearer_auth(&token)
                .query(&[
                    ("method", "food.get.v2"),
                    ("format", "json"),
                    ("food_id", &food_id.to_string()),
                ])
                .send()
        })
        .await
    }

    /// Resolve a product barcode (GTIN-13) to a FatSecret food id.
    /// Returns `None` when FatSecret reports no match (value "0").
    pub async fn find_food_id_by_barcode(&self, barcode: &str) -> Result<Option<i64>, reqwest::Error> {
        let token = self.get_token().await?;
        let json: serde_json::Value = self.send_with_retry(|| {
            self.client.get("https://platform.fatsecret.com/rest/server.api")
                .bearer_auth(&token)
                .query(&[
                    ("method", "food.find_id_for_barcode"),
                    ("format", "json"),
                    ("barcode", barcode),
                ])
                .send()
        })
        .await?;

        if let Some(food_id_obj) = json.get("food_id") {
            if let Some(val_str) = food_id_obj.get("value").and_then(|v| v.as_str()) {
                let id = val_str.parse::<i64>().unwrap_or(0);
                return Ok(if id > 0 { Some(id) } else { None });
            }
        }
        Ok(None)
    }
}

// ── Helper for inconsistent array representation in FatSecret JSON ──────────

fn deserialize_maybe_array<'de, T, D>(deserializer: D) -> Result<Option<Vec<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    use serde::de::IntoDeserializer;
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::Array(arr) => {
            let vec: Result<Vec<T>, _> = arr.into_iter()
                .map(|val| T::deserialize(val.into_deserializer()))
                .collect();
            vec.map(Some).map_err(serde::de::Error::custom)
        }
        other => {
            let single = T::deserialize(other.into_deserializer()).map_err(serde::de::Error::custom)?;
            Ok(Some(vec![single]))
        }
    }
}

// ── Deserialization Structs ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct FSRecipeSearchWrapper {
    pub recipes: Option<FSRecipeSearchBody>,
}

#[derive(Debug, Deserialize)]
pub struct FSRecipeSearchBody {
    #[serde(deserialize_with = "deserialize_maybe_array", default)]
    pub recipe: Option<Vec<FSRecipeListItem>>,
    pub total_results: Option<String>,
    pub max_results: Option<String>,
    pub page_number: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct FSRecipeListItem {
    pub recipe_id: String,
    pub recipe_name: String,
    pub recipe_description: Option<String>,
    pub recipe_image: Option<String>,
    pub recipe_url: Option<String>,
    pub recipe_nutrition: Option<FSRecipeNutritionListItem>,
}

#[derive(Debug, Deserialize)]
pub struct FSRecipeNutritionListItem {
    pub calories: Option<String>,
    pub carbohydrate: Option<String>,
    pub protein: Option<String>,
    pub fat: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct FSRecipeDetailWrapper {
    pub recipe: FSRecipeDetail,
}

#[derive(Debug, Deserialize)]
pub struct FSRecipeDetail {
    pub recipe_id: String,
    pub recipe_name: String,
    pub recipe_description: Option<String>,
    pub recipe_image: Option<String>,
    pub recipe_url: Option<String>,
    pub servings: Option<String>,
    pub prep_time_min: Option<String>,
    pub cook_time_min: Option<String>,
    pub directions: Option<FSDirections>,
    pub ingredients: Option<FSIngredients>,
    pub recipe_nutrition: Option<FSRecipeNutritionDetail>,
}

#[derive(Debug, Deserialize)]
pub struct FSDirections {
    #[serde(deserialize_with = "deserialize_maybe_array", default)]
    pub direction: Option<Vec<FSDirectionListItem>>,
}

#[derive(Debug, Deserialize)]
pub struct FSDirectionListItem {
    pub direction_number: String,
    pub direction_description: String,
}

#[derive(Debug, Deserialize)]
pub struct FSIngredients {
    #[serde(deserialize_with = "deserialize_maybe_array", default)]
    pub ingredient: Option<Vec<FSIngredientListItem>>,
}

#[derive(Debug, Deserialize)]
pub struct FSIngredientListItem {
    pub food_id: String,
    pub food_name: String,
    pub number_of_units: Option<String>,
    pub measurement_description: Option<String>,
    pub ingredient_description: String,
}

#[derive(Debug, Deserialize)]
pub struct FSRecipeNutritionDetail {
    pub calories: Option<String>,
    pub protein: Option<String>,
    pub carbohydrate: Option<String>,
    pub fat: Option<String>,
    pub fiber: Option<String>,
    pub sugar: Option<String>,
    pub sodium: Option<String>,
    pub saturated_fat: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct FSFoodsSearchWrapper {
    pub foods: Option<FSFoodsSearchBody>,
}

#[derive(Debug, Deserialize)]
pub struct FSFoodsSearchBody {
    #[serde(deserialize_with = "deserialize_maybe_array", default)]
    pub food: Option<Vec<FSFoodListItem>>,
    pub total_results: Option<String>,
    pub max_results: Option<String>,
    pub page_number: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct FSFoodListItem {
    pub food_id: String,
    pub food_name: String,
    pub food_type: String,
    pub brand_name: Option<String>,
    pub food_url: Option<String>,
    pub food_description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct FSFoodDetailWrapper {
    pub food: FSFoodDetail,
}

#[derive(Debug, Deserialize)]
pub struct FSBarcodeWrapper {
    pub food_id: FSBarcodeId,
}

#[derive(Debug, Deserialize)]
pub struct FSBarcodeId {
    pub value: String,
}

#[derive(Debug, Deserialize)]
pub struct FSFoodDetail {
    pub food_id: String,
    pub food_name: String,
    pub food_type: String,
    pub brand_name: Option<String>,
    pub servings: Option<FSServings>,
}

#[derive(Debug, Deserialize)]
pub struct FSServings {
    #[serde(deserialize_with = "deserialize_maybe_array", default)]
    pub serving: Option<Vec<FSServingItem>>,
}

#[derive(Debug, Deserialize)]
pub struct FSServingItem {
    pub serving_id: String,
    pub serving_description: String,
    pub metric_serving_amount: Option<String>,
    pub metric_serving_unit: Option<String>,
    pub measurement_description: String,
    pub calories: Option<String>,
    pub protein: Option<String>,
    pub carbohydrate: Option<String>,
    pub fat: Option<String>,
    pub fiber: Option<String>,
    pub sugar: Option<String>,
    pub sodium: Option<String>,
    pub saturated_fat: Option<String>,
    pub cholesterol: Option<String>,
}
