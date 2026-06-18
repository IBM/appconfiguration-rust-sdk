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

use std::{
    collections::HashMap,
    env,
    sync::{Arc, Mutex},
    thread,
};

use dotenvy::dotenv;
use ibm_appconfiguration_rust_sdk::{
    AppConfiguration, AppConfigurationContextOptions, ConfigurationProvider, Entity, Feature,
    Property, RuntimeEvent, RuntimeEventKind, Value,
};
use std::error::Error;

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

    fn get_attributes(&self) -> HashMap<String, Value> {
        HashMap::from_iter(vec![
            ("city".to_string(), Value::from(self.city.clone())),
            ("radius".to_string(), Value::from(self.radius as u64)),
        ])
    }
}

fn evaluate_and_print(
    client: &AppConfiguration,
    feature_id: &str,
    property_id: &str,
    entity: &CustomerEntity,
) {
    match client.get_feature(feature_id) {
        Ok(feature) => {
            println!(
                "  Feature  : '{}' (id: {})",
                feature.get_feature_name().unwrap_or_default(),
                feature_id
            );
            println!("  Enabled  : {}", feature.is_enabled().unwrap_or(false));
            match feature.get_current_value(entity) {
                Ok(result) => println!("  Value    : {:?}", result.value),
                Err(e) => println!("  Value    : <error: {e}>"),
            }
        }
        Err(e) => println!("  Feature '{feature_id}': <error: {e}>"),
    }

    match client.get_property(property_id) {
        Ok(property) => {
            println!(
                "  Property : '{}' (id: {})",
                property.get_property_name().unwrap_or_default(),
                property_id
            );
            match property.get_current_value(entity) {
                Ok(result) => println!("  Value    : {:?}", result.value),
                Err(e) => println!("  Value    : <error: {e}>"),
            }
        }
        Err(e) => println!("  Property '{property_id}': <error: {e}>"),
    }
}

fn main() -> std::result::Result<(), Box<dyn Error>> {
    dotenv().ok();
    let region = env::var("REGION").expect("REGION should be set.");
    let guid = env::var("GUID").expect("GUID should be set.");
    let apikey = env::var("APIKEY").expect("APIKEY should be set.");
    let collection_id = env::var("COLLECTION_ID").expect("COLLECTION_ID should be set.");
    let environment_id = env::var("ENVIRONMENT_ID").expect("ENVIRONMENT_ID should be set.");
    let feature_id = env::var("FEATURE_ID").expect("FEATURE_ID should be set.");
    let property_id = env::var("PROPERTY_ID").expect("PROPERTY_ID should be set.");

    // Share the client via Arc<Mutex> so the listener closure (invoked from a
    // background thread) can lock it and call get_feature / get_property.
    let client = Arc::new(Mutex::new(AppConfiguration::new()));

    {
        let mut c = client.lock().unwrap();
        c.init(&region, &guid, &apikey)?;
        c.set_context(
            &collection_id,
            &environment_id,
            AppConfigurationContextOptions::default(),
        )?;
    }

    println!("Waiting to get online...");
    client.lock().unwrap().wait_until_online();
    println!("Online!\n");

    // ── Event listener ───────────────────────────────────────────────────────
    // Clone the Arc so the closure owns its own handle to the shared client.
    // The closure is called by the SDK background thread every time it
    // successfully re-fetches configuration after a WebSocket change signal.
    let listener_client = Arc::clone(&client);
    let cb_feature_id = feature_id.clone();
    let cb_property_id = property_id.clone();

    client
        .lock()
        .unwrap()
        .emitter()
        .on(Arc::new(move |event: RuntimeEvent| {
            if event.kind != RuntimeEventKind::RefreshSuccess {
                return;
            }

            println!("[event] New configuration fetched from server — re-evaluating:");

            let entity = CustomerEntity {
                id: "user123".to_string(),
                city: "Bangalore".to_string(),
                radius: 60,
            };

            // Lock the shared client to read the freshly-fetched configuration.
            if let Ok(c) = listener_client.lock() {
                evaluate_and_print(&c, &cb_feature_id, &cb_property_id, &entity);
            }
        }))?;

    println!("Waiting for configuration changes (WebSocket). Press Ctrl-C to quit.\n");

    // Main thread parks indefinitely; all output comes from the listener above.
    loop {
        thread::park();
    }
}
