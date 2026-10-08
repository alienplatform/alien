# SetupFingerprintInfo

## Example Usage

```typescript
import { SetupFingerprintInfo } from "@alienplatform/platform-api/models";

let value: SetupFingerprintInfo = {
  target: "<value>",
  fingerprint: "<value>",
  version: 572115,
};
```

## Fields

| Field                                                         | Type                                                          | Required                                                      | Description                                                   |
| ------------------------------------------------------------- | ------------------------------------------------------------- | ------------------------------------------------------------- | ------------------------------------------------------------- |
| `target`                                                      | *string*                                                      | :heavy_check_mark:                                            | Stable target key for the setup contract, e.g. aws/us-east-1  |
| `fingerprint`                                                 | *string*                                                      | :heavy_check_mark:                                            | Deterministic setup contract fingerprint for one setup target |
| `version`                                                     | *number*                                                      | :heavy_check_mark:                                            | Setup fingerprint algorithm version                           |