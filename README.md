[![Crates.io](https://img.shields.io/crates/v/appconfiguration.svg)](https://crates.io/crates/appconfiguration)
[![Workflow Status](https://github.com/IBM/appconfiguration-rust-sdk/workflows/main/badge.svg)](https://github.com/IBM/appconfiguration-rust-sdk/actions?query=workflow%3A%22main%22)

# IBM Cloud App Configuration Rust SDK

The IBM Cloud App Configuration Rust SDK is used to perform feature flag and property
evaluation based on the configuration on IBM Cloud App Configuration service.

## Overview

[IBM Cloud App Configuration](https://cloud.ibm.com/docs/app-configuration) is a centralized
feature management and configuration service on [IBM Cloud](https://www.cloud.ibm.com) for
use with web and mobile applications, microservices, and distributed environments.

Instrument your applications with App Configuration Rust SDK, and use the App Configuration
dashboard, API or CLI to define feature flags or properties, organized into collections and
targeted to segments. Change feature flag states in the cloud to activate or deactivate features
in your application or environment, when required. You can also manage the properties for distributed
applications centrally.

## Pre-requisites

You will need the `apikey`, `region` and `guid` for the AppConfiguration you want to connect to
from your [IBMCloud account](https://cloud.ibm.com/).

## Usage

**Note.-** This crate is still under heavy development. Breaking changes are expected.

### Recommended top-level SDK flow

The Rust SDK now provides a Node-style top-level wrapper through [`AppConfigurationSdk`](appconfiguration-rust-sdk/src/client/app_configuration_sdk.rs:34), following the same high-level pattern as the Node SDK:
- create the SDK
- optionally configure private-endpoint behavior
- call `init`
- call `set_context`
- evaluate features and properties

```rust
use appconfiguration::{
    AppConfigurationContextOptions, AppConfigurationSdk, ConfigurationProvider, Entity, Result,
    Value,
};

// Create the top-level SDK wrapper
let mut sdk = AppConfigurationSdk::new();
sdk.use_private_endpoint(false);
sdk.init(region, guid, apikey)?;
sdk.set_context(
    collection_id,
    environment_id,
    AppConfigurationContextOptions::default(),
)?;

// Get the feature you want to evaluate for your entities
let feature = sdk.get_feature("AB_testing_feature")?;

// Evaluate feature value for each one of your entities
let user = MyEntity; // Implements Entity

let value_for_this_user = feature.get_value(&user)?.try_into()?;
if value_for_this_user {
    println!("Feature {} is active for user {}", feature.get_name()?, user.get_id());
} else {
    println!("User {} keeps using the legacy workflow", user.get_id());
}
```

### Lower-level direct client flow

The lower-level constructor is still available for callers that want to create the IBM Cloud client directly.

```rust
use appconfiguration::{
    AppConfigurationClientIBMCloud, ConfigurationId, ConfigurationProvider, Entity, OfflineMode,
    Result, Value,
};

let configuration = ConfigurationId::new(guid, environment_id, collection_id);
let client = AppConfigurationClientIBMCloud::new(
    &apikey,
    &region,
    configuration,
    OfflineMode::Fail,
    false,
)?;

let feature = client.get_feature("AB_testing_feature")?;
let user = MyEntity;

let value_for_this_user = feature.get_value(&user)?.try_into()?;
if value_for_this_user {
    println!("Feature {} is active for user {}", feature.get_name()?, user.get_id());
} else {
    println!("User {} keeps using the legacy workflow", user.get_id());
}
```


## License

This project is released under the Apache 2.0 license. The license's full text can be found
in [LICENSE](./LICENSE).
