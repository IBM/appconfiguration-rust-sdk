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

pub mod errors;
pub(crate) mod http_client;
mod token_provider;

pub(crate) use http_client::ServerClientImpl;
pub use http_client::ServiceAddress;
pub(crate) use http_client::ServiceAddressProtocol;
pub(crate) use token_provider::IBMCloudTokenProvider;
pub use token_provider::TokenProvider;
pub(crate) mod live_configuration;

pub use errors::NetworkError;
pub type NetworkResult<T> = std::result::Result<T, NetworkError>;

pub(crate) mod serialization; // FIXME: Make this module private to 'network'
