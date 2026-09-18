# SupportedCloudRegions

## Example Usage

```typescript
import { SupportedCloudRegions } from "@alienplatform/platform-api/models";

let value: SupportedCloudRegions = {
  aws: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  gcp: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  azure: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `aws`                                                | *string*[]                                           | :heavy_check_mark:                                   | AWS regions supported by this Alien environment.     |
| `gcp`                                                | *string*[]                                           | :heavy_check_mark:                                   | GCP regions supported by this Alien environment.     |
| `azure`                                              | *string*[]                                           | :heavy_check_mark:                                   | Azure locations supported by this Alien environment. |