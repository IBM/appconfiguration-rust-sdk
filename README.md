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

Create your client with the context (environment and collection) you want to connect to

```rust
use appconfiguration_rust_sdk::{AppConfigurationClient, Entity, Result, Value, Feature};

// Create the client connecting to the server
let client = AppConfigurationClient::new(&apikey, &region, &guid, &environment_id, &collection_id)?;

// Get the feature you want to evaluate for your entities
let feature = client.get_feature("AB_testing_feature")?;

// Evaluate feature value for each one of your entities
let user = MyEntity; // Implements Entity

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
