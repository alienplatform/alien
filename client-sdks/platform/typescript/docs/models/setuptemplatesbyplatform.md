# SetupTemplatesByPlatform

The setup templates `alien release --sign` rendered, by platform and template name, as base64 of the exact bytes. Required for every template a signed statement names, and only for those; each sha256 must equal the statement's digest. Protected deployments are served these bytes.

## Example Usage

```typescript
import { SetupTemplatesByPlatform } from "@alienplatform/platform-api/models";

let value: SetupTemplatesByPlatform = {};
```

## Fields

| Field                    | Type                     | Required                 | Description              |
| ------------------------ | ------------------------ | ------------------------ | ------------------------ |
| `aws`                    | Record<string, *string*> | :heavy_minus_sign:       | N/A                      |
| `gcp`                    | Record<string, *string*> | :heavy_minus_sign:       | N/A                      |
| `azure`                  | Record<string, *string*> | :heavy_minus_sign:       | N/A                      |
| `kubernetes`             | Record<string, *string*> | :heavy_minus_sign:       | N/A                      |
| `machines`               | Record<string, *string*> | :heavy_minus_sign:       | N/A                      |
| `local`                  | Record<string, *string*> | :heavy_minus_sign:       | N/A                      |
| `test`                   | Record<string, *string*> | :heavy_minus_sign:       | N/A                      |