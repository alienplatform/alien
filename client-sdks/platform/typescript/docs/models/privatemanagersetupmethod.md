# PrivateManagerSetupMethod

Optional setup method. Defaults to cloudformation for AWS, google-oauth for GCP, and terraform for Azure.

## Example Usage

```typescript
import { PrivateManagerSetupMethod } from "@alienplatform/platform-api/models";

let value: PrivateManagerSetupMethod = "cloudformation";
```

## Values

```typescript
"cloudformation" | "google-oauth" | "terraform"
```