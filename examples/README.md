# demo

A live demo that connects to IBM App Configuration and **reacts to flag/property changes in
real time** — no polling, no restart needed.

The demo uses the SDK's WebSocket-backed event listener. When you toggle a feature flag or
update a property value in the IBM Cloud App Configuration dashboard, the server sends a
change notification over the open WebSocket. The SDK re-fetches the full configuration and
fires a `RefreshSuccess` event; the listener prints the new evaluated values immediately.

## Setup

### 1. Create a `.env` file

Place it in the **workspace root** (the same directory as `Cargo.toml`):

```ini
REGION=us-south
GUID=xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
APIKEY=your-ibm-cloud-api-key
COLLECTION_ID=my-collection
ENVIRONMENT_ID=dev
FEATURE_ID=my-feature-flag-id
PROPERTY_ID=my-property-id
```

| Variable | Where to find it |
|---|---|
| `REGION` | Service instance region (e.g. `us-south`, `eu-de`) |
| `GUID` | Service instance GUID — **Manage → Service credentials** |
| `APIKEY` | IBM Cloud API key — **Manage → Access (IAM) → API keys** |
| `COLLECTION_ID` | App Configuration dashboard → Collections |
| `ENVIRONMENT_ID` | App Configuration dashboard → Environments |
| `FEATURE_ID` | ID of the feature flag you want to watch |
| `PROPERTY_ID` | ID of the property you want to watch |

### 2. Run

```bash
cargo run --example demo
```

Expected startup output:

```
Waiting to get online...
Online!

Waiting for configuration changes (WebSocket). Press Ctrl-C to quit.
```

### 3. Trigger a change

Go to the IBM Cloud App Configuration dashboard, toggle the feature flag or update the
property value, then save. Within a few seconds the terminal prints:

```
[event] New configuration fetched from server — re-evaluating:
  Feature  : 'My Feature' (id: my-feature-flag-id)
  Enabled  : true
  Value    : Boolean(true)
  Property : 'My Property' (id: my-property-id)
  Value    : String("new-value")
```

Press **`Ctrl+C`** to stop.

---

## Code walkthrough

### Entity — targeting attributes

```rust
struct CustomerEntity {
    id:     String,
    city:   String,
    radius: u32,
}
```

Implements the [`Entity`](../src/entity.rs) trait. The `city` and `radius` attributes are
matched against the targeting segment rules you define in the IBM Cloud dashboard. The entity
used in this demo is hard-coded to `id = "user123"`, `city = "Bangalore"`, `radius = 60` —
change these to see different targeting outcomes.

### Client setup — `Arc<Mutex<AppConfiguration>>`

```rust
let client = Arc::new(Mutex::new(AppConfiguration::new()));

{
    let mut c = client.lock().unwrap();
    c.init(&region, &guid, &apikey)?;
    c.set_context(&collection_id, &environment_id, AppConfigurationContextOptions::default())?;
}
```

The client is wrapped in `Arc<Mutex<>>` so it can be shared safely between the **main
thread** and the **SDK background thread** that invokes the listener closure.

The `init` + `set_context` calls are grouped in a scoped block `{ … }` so the
`MutexGuard` is dropped (lock released) before `wait_until_online()` is called.

> **Why not `let mut client = AppConfiguration::new()`?**
> The listener closure must be `'static + Send + Sync`. A bare reference to `client`
> cannot cross thread boundaries — the borrow checker will reject it. `Arc<Mutex<>>` solves
> this: `Arc` gives shared ownership, `Mutex` gives safe exclusive access.

### Waiting for the initial config fetch

```rust
client.lock().unwrap().wait_until_online();
```

Blocks the main thread until the background thread has connected to the WebSocket **and**
successfully fetched the initial configuration from the server (up to 30 s timeout).
The `MutexGuard` is a temporary — it is dropped at the `;` so the lock is released
before the next line runs.

### Registering the event listener

```rust
let listener_client = Arc::clone(&client);   // ← closure owns its own Arc handle
let cb_feature_id   = feature_id.clone();
let cb_property_id  = property_id.clone();

client.lock().unwrap().emitter().on(Arc::new(move |event: RuntimeEvent| {
    if event.kind != RuntimeEventKind::RefreshSuccess {
        return;
    }
    // ...
    if let Ok(c) = listener_client.lock() {
        evaluate_and_print(&c, &cb_feature_id, &cb_property_id, &entity);
    }
}))?;
```

Key points:

| Point | Detail |
|---|---|
| `Arc::clone(&client)` | Gives the closure its own reference-counted handle — no borrow of the original |
| `move` closure | Takes ownership of `listener_client`, `cb_feature_id`, `cb_property_id` |
| `listener_client.lock()` | Safe to call from the background thread — main thread is parked and does not hold the lock |
| Filter on `RefreshSuccess` | Ignores `Connected`, `Disconnected`, `HeartbeatTimeout`, etc. |
| `evaluate_and_print(&c, …)` | Takes `&AppConfiguration` directly — avoids any re-locking inside the helper |

#### Why filter on `RefreshSuccess` specifically?

The IBM App Configuration server sends `"test message"` as a keep-alive **heartbeat** every
~60 seconds. The SDK recognises this string and skips the config fetch — **no event is
fired**. A config-change notification carries a different payload (e.g.
`"collection_id:c1;environment_id:e1"`), which triggers a full HTTP re-fetch and, on
success, a `RefreshSuccess` event. This means the listener only fires when something
actually changed.

### `evaluate_and_print` helper

```rust
fn evaluate_and_print(client: &AppConfiguration, feature_id: &str, property_id: &str, entity: &CustomerEntity) {
    match client.get_feature(feature_id) { … }
    match client.get_property(property_id) { … }
}
```

Takes a plain `&AppConfiguration` reference (already locked by the caller). Using
`get_feature` / `get_property` (snapshot APIs) rather than proxy APIs avoids any attempt to
re-acquire the mutex while the caller already holds it.

### Main thread — parking

```rust
loop {
    thread::park();
}
```

The main thread parks indefinitely. All output is produced by the listener closure running
on the SDK background thread. `thread::park()` uses essentially zero CPU.

---

## How the WebSocket event flow works

```
IBM Cloud dashboard
  └─ user toggles flag
       └─ server sends WS notification: "collection_id:c1;environment_id:e1"
            └─ SDK background thread receives message
                 └─ message ≠ "test message"  →  HTTP GET /config
                      └─ new Configuration stored in memory
                           └─ RefreshSuccess event fired
                                └─ listener closure runs on background thread
                                     └─ evaluate_and_print() prints new values
```

Heartbeat path (no output):

```
server sends: "test message"
  └─ SDK recognises heartbeat → skips fetch → no event fired
```
