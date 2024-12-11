// (C) Copyright IBM Corp. 2024.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::{collections::HashMap, env, thread, time::Duration};

use appconfiguration_rust_sdk::{client::AppConfigurationClient, AttrValue, Entity, Feature, Property};
use dotenvy::dotenv;
use std::error::Error;

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Debug)]
struct CustomerEntity {
    id: String,
    city: String,
    radius: u32,
}

impl Entity for CustomerEntity {
    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn get_attributes(&self) -> HashMap<String, AttrValue> {
        use AttrValue;

        HashMap::from_iter(vec![
            ("city".to_string(), AttrValue::String(self.city.clone())),
            ("radius".to_string(), AttrValue::Numeric(self.radius as f64)),
        ])
    }
}

fn main() -> Result<()> {
    dotenv().ok();
    let region = env::var("REGION").expect("REGION should be set.");
    let guid = env::var("GUID").expect("GUID should be set.");
    let apikey = env::var("APIKEY").expect("APIKEY should be set.");
    let collection_id = env::var("COLLECTION_ID").expect("COLLECTION_ID should be set.");
    let environment_id = env::var("ENVIRONMENT_ID").expect("ENVIRONMENT_ID should be set.");
    let feature_id = env::var("FEATURE_ID").expect("FEATURE_ID should be set.");
    let property_id = env::var("PROPERTY_ID").expect("PROPERTY_ID should be set.");

    let client =
        AppConfigurationClient::new(&apikey, &region, &guid, &environment_id, &collection_id)?;

    let entity = CustomerEntity {
        id: "user123".to_string(),
        city: "Bangalore".to_string(),
        radius: 60,
    };

    println!("The information is displayed every 5 seconds.");
    println!("Try changing the configuraiton in the App Configuration instances.");

    loop {
        println!("\n\nFEATURE FLAG OPERATIONS\n");

        match client.get_feature_proxy(&feature_id) {
            Ok(feature) => {
                println!("Feature name: {}", feature.get_name()?);
                println!("Feature id: {}", feature.get_id());
                println!("Feature data type: {}", feature.get_data_type()?);
                println!("Is feature enabled: {}", feature.is_enabled()?);
                println!(
                    "Feature evaluated value is: {:?}",
                    feature.get_value(&entity)?
                );
            }
            Err(error) => {
                println!("There was an error getting the Feature Flag. Error {error}",);
            }
        }

        println!("\n\nPROPERTY OPERATIONS\n");
        match client.get_property_proxy(&property_id) {
            Ok(property) => {
                println!("Property name: {}", property.get_name()?);
                println!("Property id: {}", property.get_id());
                println!("Property data type: {}", property.get_data_type()?);
                println!(
                    "Property evaluated value is: {:?}",
                    property.get_value(&entity)?
                );
            }
            Err(error) => {
                println!("There was an error getting the Property. Error {error}",);
            }
        }

        thread::sleep(Duration::from_secs(5));
    }
}
