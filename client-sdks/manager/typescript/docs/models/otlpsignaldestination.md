# OtlpSignalDestination

## Example Usage

```typescript
import { OtlpSignalDestination } from "@alienplatform/manager-api/models";

let value: OtlpSignalDestination = {
  authentication: {
    tokenEnvironmentVariable: "<value>",
    type: "bearer",
  },
  endpoint: {
    type: "url",
    url: "https://french-deduction.com",
  },
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `allowInsecureHttp`                                                              | *boolean*                                                                        | :heavy_minus_sign:                                                               | HTTP must be explicitly approved, for example for a local development collector. |
| `authentication`                                                                 | *models.OtlpDestinationAuthentication*                                           | :heavy_check_mark:                                                               | N/A                                                                              |
| `endpoint`                                                                       | *models.OtlpDestinationEndpoint*                                                 | :heavy_check_mark:                                                               | N/A                                                                              |
