# ExtendReleaseInfoPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { ExtendReleaseInfoPlatforms } from "@alienplatform/platform-api/models";

let value: ExtendReleaseInfoPlatforms = {};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `aws`                                                                  | [models.ExtendReleaseInfoAw](../models/extendreleaseinfoaw.md)[]       | :heavy_minus_sign:                                                     | AWS permission configurations                                          |
| `azure`                                                                | [models.ExtendReleaseInfoAzure](../models/extendreleaseinfoazure.md)[] | :heavy_minus_sign:                                                     | Azure permission configurations                                        |
| `gcp`                                                                  | [models.ExtendReleaseInfoGcp](../models/extendreleaseinfogcp.md)[]     | :heavy_minus_sign:                                                     | GCP permission configurations                                          |