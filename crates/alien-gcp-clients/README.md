# alien-gcp-clients

Custom HTTP client for GCP APIs. Makes direct API calls using `reqwest` with JWT-generated Bearer tokens — not a wrapper around the official Google Cloud SDK.

Services: Cloud Run, Cloud Storage (GCS), Firestore, Pub/Sub, IAM, Artifact Registry, Cloud Build, Cloud Scheduler, Compute Engine, Resource Manager, Secret Manager, Service Usage.

Token caching with 45-minute TTL. Trait-based API design with `mockall` support.
