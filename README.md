# IBM Cloud App Configuration Rust SDK

IBM Cloud App Configuration SDK is used to perform feature flag and property evaluation and track custom metrics for Experimentation based on the configuration on IBM Cloud App Configuration service.

[![Crates.io](https://img.shields.io/crates/v/ibm-appconfiguration-rust-sdk.svg)](https://crates.io/crates/ibm-appconfiguration-rust-sdk)
[![Workflow Status](https://github.com/IBM/appconfiguration-rust-sdk/workflows/main/badge.svg)](https://github.com/IBM/appconfiguration-rust-sdk/actions?query=workflow%3A%22main%22)

## Table of Contents

- [Overview](#overview)
- [Installation](#installation)
- [Import the SDK](#import-the-sdk)
- [Usage](#usage)
- [Adding URLs to your allowlist](#adding-urls-to-your-allowlist)
- [License](#license)

## Overview

IBM Cloud App Configuration is a centralized feature management and configuration service
on [IBM Cloud](https://www.cloud.ibm.com) for use with web and mobile applications, microservices, and distributed
environments.

Instrument your applications with App Configuration Rust SDK, and use the App Configuration
dashboard, CLI or API to define feature flags or properties, organized into collections and
targeted to segments. Toggle feature flag states in the cloud to activate or deactivate features
in your application or environment, when required. Run experiments and measure the effect of
feature flags on end users by tracking custom metrics. You can also manage the properties for
distributed applications centrally.

## Installation

Add the SDK to your `Cargo.toml`:

```toml
[dependencies]
ibm-appconfiguration-rust-sdk = "0.1.0-rc.0"
```

## Import the SDK

```rust
use ibm_appconfiguration_rust_sdk::{
    AppConfiguration, AppConfigurationContextOptions,
    ConfigurationProvider, Entity, Value,
};
```

## Usage

Initialize the SDK to connect with your App Configuration service instance.

```rust
use ibm_appconfiguration_rust_sdk::{
    AppConfiguration, AppConfigurationContextOptions,
    ConfigurationProvider, Entity, Value,
};

let region = "us-south";
let guid = "<guid>";
let apikey = "<apikey>";
let collection_id = "airlines-webapp";
let environment_id = "dev";

let mut client = AppConfiguration::new();
client.init(&region, &guid, &apikey)?;
client.set_context(
    &collection_id,
    &environment_id,
    AppConfigurationContextOptions::default(),
)?;

// Block until the background thread has fetched configuration from the server
client.wait_until_online();
```

> ⚠️ It is expected that initialization to be done **only once**.

After the SDK is initialized successfully, the feature flags & properties can be retrieved using the `client` as shown in the below code snippet.

<details><summary>Expand to view the example snippet</summary>

```rust
// Get feature
match client.get_feature("online-check-in") {
    Ok(feature) => {
        let result = feature.get_current_value(&entity)?;
        println!("{:?}", result.value);
    }
    Err(e) => eprintln!("Feature not found: {e}"),
}

// Get property
match client.get_property("check-in-charges") {
    Ok(property) => {
        let result = property.get_current_value(&entity)?;
        println!("{:?}", result.value);
    }
    Err(e) => eprintln!("Property not found: {e}"),
}
```
</details>

where,
- **region**: Region name where the App Configuration service instance is created.
  See list of supported locations [here](https://cloud.ibm.com/catalog/services/app-configuration). E.g. `us-south`, `au-syd`, `eu-gb`, `us-east`, `eu-de`, `ca-tor`, `jp-tok`, `jp-osa` etc.
- **guid**: Instance Id of the App Configuration service. Obtain it from the service credentials section of the App Configuration dashboard.
- **apikey**: ApiKey of the App Configuration service. Obtain it from the service credentials section of the App Configuration dashboard.
- **collection_id**: Id of the collection created in App Configuration service instance under the **Collections** section.
- **environment_id**: Id of the environment created in App Configuration service instance under the **Environments** section.

### Connect using private network connection (optional)

Set the SDK to connect to App Configuration service by using a private endpoint that is accessible only through the IBM Cloud private network.

```rust
client.use_private_endpoint(true);
```

This must be done before calling `init()` on the SDK.

### Persistent cache (optional)

In order for your application and SDK to continue its operations even during the unlikely scenario of App Configuration service across your application restarts, you can configure the SDK to work using a persistent cache. The SDK uses the persistent cache to store the App Configuration data that will be available across your application restarts.

```rust
use std::path::PathBuf;
use ibm_appconfiguration_rust_sdk::AppConfigurationContextOptions;

let options = AppConfigurationContextOptions::try_new(
    Some(PathBuf::from("/var/lib/myapp/cache")),
    None,
    true,
)?;

client.set_context(&collection_id, &environment_id, options)?;
```

- **persistent_cache_directory**: Absolute path to a directory which has read & write permission for the user. The SDK will create a file — `appconfiguration.json` in the specified directory, and it will be used as the persistent cache to store the App Configuration service information.

When persistent cache is enabled, the SDK will keep the last known good configuration at the persistent cache. In the case of App Configuration server being unreachable, the latest configurations at the persistent cache is loaded to the application to continue working.

Please ensure that the cache file is not lost or deleted in any case. For example, consider the case when a Kubernetes pod is restarted and the cache file (`appconfiguration.json`) was stored in an ephemeral volume of the pod. As the pod gets restarted, Kubernetes destroys the ephemeral volume, and the cache file gets deleted. Make sure that the cache file created by the SDK is always stored in a persistent volume by providing the correct absolute path of the persistent directory.

### Bootstrap file (optional)

The SDK is also designed to serve configurations, perform feature flag & property evaluations without being connected to App Configuration service.

```rust
use std::path::PathBuf;
use ibm_appconfiguration_rust_sdk::AppConfigurationContextOptions;

let options = AppConfigurationContextOptions::try_new(
    None,
    Some(PathBuf::from("saflights/flights.json")),
    false,
)?;

client.set_context(&collection_id, &environment_id, options)?;
```

This will return an error if the given `bootstrap_file` is not found or if the JSON cannot be parsed.

- **bootstrap_file**: Absolute path of the JSON file which contains configuration details. Make sure to provide a valid JSON file. You can generate this file using the `ibmcloud ac export` command of the IBM Cloud App Configuration CLI.
- **live_config_update_enabled**: Live configuration update from the server. Set this value to `false` if new configuration values should not be fetched from the server.

## Get single feature

```rust
match client.get_feature("online-check-in") {
    Ok(feature) => {
        println!("Feature Name: {}", feature.get_feature_name()?);
        println!("Feature Id: {}", feature.get_feature_id()?);
        println!("Feature Type: {}", feature.get_feature_data_type()?);
        if feature.is_enabled()? {
            // feature flag is enabled
        } else {
            // feature flag is disabled
        }
    }
    Err(e) => eprintln!("Invalid feature id: {e}"),
}
```

## Get all features

```rust
let feature_ids = client.get_feature_ids()?;
for id in &feature_ids {
    if let Ok(feature) = client.get_feature(id) {
        println!("Feature Name: {}", feature.get_feature_name()?);
        println!("Feature Id: {}", feature.get_feature_id()?);
        println!("Feature Type: {}", feature.get_feature_data_type()?);
        println!("Is feature enabled? {}", feature.is_enabled()?);
    }
}
```

## Evaluate a feature

Use the `feature.get_current_value(&entity)` method to evaluate the value of the feature flag. This method returns a [`FeatureEvaluationResult`](src/models/evaluation_result.rs) containing the evaluated value, enabled status and evaluation details.

```rust
use std::collections::HashMap;
use ibm_appconfiguration_rust_sdk::{Entity, Value};

struct MyEntity {
    id: String,
    city: String,
    country: String,
}

impl Entity for MyEntity {
    fn get_id(&self) -> String { self.id.clone() }
    fn get_attributes(&self) -> HashMap<String, Value> {
        HashMap::from([
            ("city".to_string(),    Value::from(self.city.clone())),
            ("country".to_string(), Value::from(self.country.clone())),
        ])
    }
}

let entity = MyEntity {
    id: "john_doe".to_string(),
    city: "Bangalore".to_string(),
    country: "India".to_string(),
};

let feature = client.get_feature("online-check-in")?;
let result = feature.get_current_value(&entity)?;

println!("{:?}", result.value);       // Evaluated value (Boolean, Numeric, or String)
println!("{}", result.is_enabled);    // Enabled status
println!("{:?}", result.details);     // Detailed evaluation info

// result.details fields:
println!("{}", result.details.value_type);                   // e.g. "DISABLED_VALUE"
println!("{}", result.details.reason);                       // e.g. "Disabled value of the feature flag since the feature flag is disabled."
println!("{:?}", result.details.segment_name);               // Some("segment_name") or None
println!("{:?}", result.details.rollout_percentage_applied); // Some(true/false) or None
```

- **entity_id**: Id of the Entity. This will be a string identifier related to the Entity against which the feature is evaluated. For example, an entity might be an instance of an app that runs on a mobile device, a microservice that runs on the cloud, or a component of infrastructure that runs that microservice. For any entity to interact with App Configuration, it must provide a unique entity ID.
- **entity_attributes**: A `HashMap` consisting of the attribute name and their values that defines the specified entity. This is optional if the feature flag is not configured with any targeting definition. If targeting is configured, then entity attributes should be provided for the rule evaluation. An attribute is a parameter used to define a segment. The SDK uses the attribute values to determine if the specified entity satisfies the targeting rules, and returns the appropriate feature flag value.

## Send custom metrics

Record custom metrics for experiments using the `track` method. Calling track will queue the metric event, which will be sent in batches to the App Configuration servers.

```rust
use ibm_appconfiguration_rust_sdk::AppConfigurationClient;

client.track("event_key", "entity_id")?;
```

where:
- **event_key**: The event key for the metric associated with the running experiment. The event key in your metric and the event key in your code must match exactly.

> **Note:** Custom metric tracking (`track`) is currently a no-op placeholder. It will be replaced with a real implementation in a future release.

## Get single property

```rust
match client.get_property("check-in-charges") {
    Ok(property) => {
        println!("Property Name: {}", property.get_property_name()?);
        println!("Property Id: {}", property.get_property_id()?);
        println!("Property Type: {}", property.get_property_data_type()?);
    }
    Err(e) => eprintln!("Invalid property id: {e}"),
}
```

## Get all properties

```rust
let property_ids = client.get_property_ids()?;
for id in &property_ids {
    if let Ok(property) = client.get_property(id) {
        println!("Property Name: {}", property.get_property_name()?);
        println!("Property Id: {}", property.get_property_id()?);
        println!("Property Type: {}", property.get_property_data_type()?);
    }
}
```

## Evaluate a property

Use the `property.get_current_value(&entity)` method to evaluate the value of the property. This method returns a [`PropertyEvaluationResult`](src/models/evaluation_result.rs) containing the evaluated value and evaluation details.

```rust
let entity = MyEntity {
    id: "john_doe".to_string(),
    city: "Bangalore".to_string(),
    country: "India".to_string(),
};

let property = client.get_property("check-in-charges")?;
let result = property.get_current_value(&entity)?;

println!("{:?}", result.value);   // Evaluated value (Boolean, Numeric, or String)
println!("{:?}", result.details); // Detailed evaluation info

// result.details fields:
println!("{}", result.details.value_type);       // e.g. "DEFAULT_VALUE"
println!("{}", result.details.reason);           // e.g. "Default value of the property."
println!("{:?}", result.details.segment_name);   // Some("segment_name") or None
```

- **entity_id**: Id of the Entity. This will be a string identifier related to the Entity against which the property is evaluated.
- **entity_attributes**: A `HashMap` consisting of the attribute name and their values that defines the specified entity. This is optional if the property is not configured with any targeting definition.

## Set listener for feature and property data changes

The SDK maintains a persistent WebSocket connection to the server. When you change a flag or property value in the IBM Cloud App Configuration dashboard, the server sends a change notification over that socket. The SDK re-fetches the full configuration and fires a [`RuntimeEventKind::RefreshSuccess`](src/client/app_configuration_client.rs:121) event to every registered listener.

**How it works:**

| Server message | SDK action |
|---|---|
| `"test message"` | Keep-alive heartbeat — ignored, no fetch |
| Any other text (e.g. `"collection_id:c1;environment_id:e1"`) | Config-change notification — fetch + fire `RefreshSuccess` |

Call [`emitter()`](src/client/app_configuration.rs:271) to obtain the [`RuntimeEventEmitter`](src/client/app_configuration_client.rs:134), then attach a closure with [`on()`](src/client/app_configuration_client.rs:143). Listeners are called on the SDK background thread.

> ⚠️ **Register your listener BEFORE `set_context()`.**
> `set_context()` spawns the background thread and seeds it with a clone of the emitter.
> `Connected` and the first `RefreshSuccess` both fire before `wait_until_online()` returns —
> a listener registered afterwards will miss them.

### Minimal example — log every config change

```rust
use std::sync::Arc;
use ibm_appconfiguration_rust_sdk::{
    AppConfiguration, AppConfigurationContextOptions,
    RuntimeEvent, RuntimeEventKind,
};

let mut client = AppConfiguration::new();
client.init(&region, &guid, &apikey)?;

// Register the listener BEFORE set_context()
client.emitter().on(Arc::new(|event: RuntimeEvent| {
    if event.kind == RuntimeEventKind::RefreshSuccess {
        println!("Configuration changed — re-evaluate your flags!");
    }
}))?;

// Background thread starts here; listener is already wired in
client.set_context(&collection_id, &environment_id, AppConfigurationContextOptions::default())?;
client.wait_until_online();
```

### Read flag values inside the callback

Listener closures must be `'static + Send + Sync`, so they cannot borrow the client directly. Wrap the client in `Arc<Mutex<AppConfiguration>>` and clone the `Arc` into the closure.

The **critical ordering rule** is:

1. Lock → `init()` → **unlock**
2. `emitter().on(...)` — lock-free (`emitter()` returns a cheap `Arc` clone)
3. Lock → `set_context()` → **unlock** ← background thread spawned here with listener already seeded
4. `wait_until_online()`

```rust
use std::sync::{Arc, Mutex};
use std::thread;
use ibm_appconfiguration_rust_sdk::{
    AppConfiguration, AppConfigurationContextOptions,
    ConfigurationProvider, RuntimeEvent, RuntimeEventKind,
};

let client = Arc::new(Mutex::new(AppConfiguration::new()));

// Step 1 — credentials
{
    let mut c = client.lock().unwrap();
    c.init(&region, &guid, &apikey)?;
} // lock released

// Step 2 — register listener (emitter() is lock-free)
let listener_client = Arc::clone(&client);
client.lock().unwrap().emitter().on(Arc::new(move |event: RuntimeEvent| {
    if event.kind != RuntimeEventKind::RefreshSuccess {
        return;
    }
    // Acquire the client to read the freshly-fetched configuration
    if let Ok(c) = listener_client.lock() {
        match c.get_feature("online-check-in") {
            Ok(f)  => println!("online-check-in enabled = {:?}", f.is_enabled()),
            Err(e) => eprintln!("error: {e}"),
        }
    }
}))?;

// Step 3 — bind context (spawns background thread with listener already wired)
{
    let mut c = client.lock().unwrap();
    c.set_context(&collection_id, &environment_id, AppConfigurationContextOptions::default())?;
} // lock released

// Step 4 — block until the first config fetch completes
client.lock().unwrap().wait_until_online();

// Main thread: park and let the listener handle all output
loop { thread::park(); }
```

> ⚠️ **Do not call `get_feature()` / `get_property()` while holding the same
> `Arc<Mutex<AppConfiguration>>` lock that the listener also tries to acquire.**
> The listener runs on the background thread. If the main thread holds the lock when
> the background thread fires the event, both threads deadlock waiting for each other.
> In the pattern above this is safe because `wait_until_online()` releases the guard
> at the `;` (it is a temporary), and the main thread then calls `thread::park()` — it
> never holds the lock when the listener fires.

### All `RuntimeEventKind` variants

```rust
use std::sync::Arc;
use ibm_appconfiguration_rust_sdk::{RuntimeEvent, RuntimeEventKind};

client.emitter().on(Arc::new(|event: RuntimeEvent| {
    match event.kind {
        RuntimeEventKind::Connected        => println!("[ws] connected to server"),
        RuntimeEventKind::Disconnected     => println!("[ws] disconnected — {:?}", event.status.offline_reason),
        RuntimeEventKind::Closed           => println!("[ws] server closed the connection"),
        RuntimeEventKind::HeartbeatTimeout => println!("[ws] heartbeat timeout — reconnecting"),
        RuntimeEventKind::RefreshSuccess   => println!("[config] new configuration fetched ✓"),
        RuntimeEventKind::RefreshFailure   => eprintln!("[config] fetch failed — {:?}", event.status),
    }
}))?;
```

| Kind | When it fires |
|---|---|
| `Connected` | WebSocket handshake succeeded |
| `Disconnected` | Connection lost (network error, server restart, etc.) |
| `Closed` | Server sent a clean WebSocket close frame |
| `HeartbeatTimeout` | No `"test message"` heartbeat received within 65 s |
| `RefreshSuccess` | Config-change notification received **and** new config fetched successfully |
| `RefreshFailure` | Config-change notification received but HTTP fetch failed |

## Implementing `Entity`

Any type that provides an ID and a map of attributes can be used with the client:

```rust
use std::collections::HashMap;
use ibm_appconfiguration_rust_sdk::{Entity, Value};

struct MyUser {
    id: String,
    city: String,
}

impl Entity for MyUser {
    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn get_attributes(&self) -> HashMap<String, Value> {
        HashMap::from([
            ("city".to_string(), Value::from(self.city.clone())),
        ])
    }
}
```

## Supported Data types

App Configuration service allows configuring the feature flag and properties in the following data types: Boolean, Numeric, String. The String data type can be of the format of a text string, JSON or YAML. The SDK processes each format accordingly as shown in the below table.

<details><summary>View Table</summary>

| **Feature or Property value** | **DataType** | **DataFormat** | **Type of data returned by `get_current_value().value`** | **Example output** |
|---|---|---|---|---|
| `true` | BOOLEAN | not applicable | `Value::Boolean` | `true` |
| `25` | NUMERIC | not applicable | `Value::Int64` / `Value::Float64` | `25` |
| `"a string text"` | STRING | TEXT | `Value::String` | `"a string text"` |
| `{"firefox":{"name":"Firefox"}}` | STRING | JSON | `Value::String` (raw JSON string) | `"{\"firefox\":{\"name\":\"Firefox\"}}"` |
| `men:\n  - John Smith` | STRING | YAML | `Value::String` (raw YAML string) | `"men:\n  - John Smith"` |

</details>

<details><summary>Feature flag</summary>

```rust
let feature = client.get_feature("json-feature")?;
println!("{}", feature.get_feature_data_type()?);   // STRING
println!("{:?}", feature.get_feature_data_format()?); // Some("JSON")

let result = feature.get_current_value(&entity)?;
// result.value is Value::String containing raw JSON
if let Value::String(raw_json) = result.value {
    let parsed: serde_json::Value = serde_json::from_str(&raw_json)?;
    println!("{}", parsed["key"]);
}

let yaml_feature = client.get_feature("yaml-feature")?;
println!("{}", yaml_feature.get_feature_data_type()?);   // STRING
println!("{:?}", yaml_feature.get_feature_data_format()?); // Some("YAML")
let result = yaml_feature.get_current_value(&entity)?;
```
</details>

<details><summary>Property</summary>

```rust
let property = client.get_property("json-property")?;
println!("{}", property.get_property_data_type()?);   // STRING
println!("{:?}", property.get_property_data_format()?); // Some("JSON")

let result = property.get_current_value(&entity)?;
if let Value::String(raw_json) = result.value {
    let parsed: serde_json::Value = serde_json::from_str(&raw_json)?;
    println!("{}", parsed["key"]);
}

let yaml_property = client.get_property("yaml-property")?;
println!("{}", yaml_property.get_property_data_type()?);   // STRING
println!("{:?}", yaml_property.get_property_data_format()?); // Some("YAML")
let result = yaml_property.get_current_value(&entity)?;
```
</details>

## Context options

[`AppConfigurationContextOptions`](src/client/app_configuration.rs:47) controls offline resilience and configuration update behaviour:

| Field | Type | Default | Description |
|---|---|---|---|
| `live_config_update_enabled` | `bool` | `true` | Poll/subscribe for live configuration updates from the server |
| `persistent_cache_directory` | `Option<PathBuf>` | `None` | Directory where a local `appconfiguration.json` cache file is written; used as fallback when the server is unreachable |
| `bootstrap_file` | `Option<PathBuf>` | `None` | Path to a static `.json` file to use as the initial (and only) configuration source when `live_config_update_enabled` is `false` |

Use [`AppConfigurationContextOptions::try_new()`](src/client/app_configuration.rs:68) to construct and validate options at the same time:

```rust
use std::path::PathBuf;
use ibm_appconfiguration_rust_sdk::AppConfigurationContextOptions;

// Persist a cache and receive live updates
let options = AppConfigurationContextOptions::try_new(
    Some(PathBuf::from("/var/cache/myapp")),
    None,
    true,
)?;
client.set_context(&collection_id, &environment_id, options)?;
```

```rust
// Fully offline: serve only from a bundled JSON file
let options = AppConfigurationContextOptions::try_new(
    None,
    Some(PathBuf::from("config/bootstrap.json")),
    false,
)?;
client.set_context(&collection_id, &environment_id, options)?;
```

## Cleanup

```rust
// Shut down background threads; preserve the on-disk cache
client.clean_up()?;

// Shut down background threads and delete the on-disk cache
client.clean_up_with_cache_clear()?;
```

## Fetching the client across other modules

Once the SDK is initialized and wrapped in an `Arc<Mutex<>>`, the client can be obtained across other modules as shown below:

```rust
use std::sync::{Arc, Mutex};
use ibm_appconfiguration_rust_sdk::{AppConfiguration, ConfigurationProvider, Feature};

// Pass the Arc<Mutex<AppConfiguration>> across module boundaries
fn use_client(client: Arc<Mutex<AppConfiguration>>) {
    let c = client.lock().unwrap();
    if let Ok(feature) = c.get_feature("online-check-in") {
        let enabled = feature.is_enabled().unwrap_or(false);
        println!("online-check-in enabled: {enabled}");
    }
}
```

## Examples

Try [this](./examples) sample application in the examples folder to learn more about feature and property evaluation.

## Adding URLs to your allowlist

This SDK requires connectivity to the internet (if bootstrap-based initialization is not done). The endpoints listed below should be reachable from the host/infrastructure where this SDK will run.

```
https://cloud.ibm.com:443
https://iam.cloud.ibm.com:443
https://{region}.apprapp.cloud.ibm.com:443
wss://{region}.apprapp.cloud.ibm.com:443
```

If opted for private endpoint by setting `client.use_private_endpoint(true)` then the allowlist will be:

```
https://cloud.ibm.com:443
https://private.iam.cloud.ibm.com:443
https://private.{region}.apprapp.cloud.ibm.com:443
wss://private.{region}.apprapp.cloud.ibm.com:443
```

where `region` is the region where your App Configuration service instance is provisioned such as `us-south`, `us-east`, `eu-gb`, `au-syd`, `eu-de`, `ca-tor`, `jp-tok`, `jp-osa` etc.

## License

This project is released under the Apache 2.0 license. The license's full text can be found
in [LICENSE](./LICENSE).
