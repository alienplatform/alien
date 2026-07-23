# PublicEndpoints

## Example Usage

```typescript
import { PublicEndpoints } from "@alienplatform/platform-api/models";

let value: PublicEndpoints = {
  url: "https://gigantic-hello.name",
  protocol: "http",
  host: "wordy-step-mother.info",
  port: 701832,
};
```

## Fields

| Field                                    | Type                                     | Required                                 | Description                              |
| ---------------------------------------- | ---------------------------------------- | ---------------------------------------- | ---------------------------------------- |
| `url`                                    | *string*                                 | :heavy_check_mark:                       | N/A                                      |
| `protocol`                               | [models.Protocol](../models/protocol.md) | :heavy_check_mark:                       | N/A                                      |
| `host`                                   | *string*                                 | :heavy_check_mark:                       | N/A                                      |
| `port`                                   | *number*                                 | :heavy_check_mark:                       | N/A                                      |
| `wildcardHost`                           | *string*                                 | :heavy_minus_sign:                       | N/A                                      |
