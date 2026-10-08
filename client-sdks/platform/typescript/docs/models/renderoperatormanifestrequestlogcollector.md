# RenderOperatorManifestRequestLogCollector

Enable the node log collector DaemonSet for raw pod logs.

## Example Usage

```typescript
import { RenderOperatorManifestRequestLogCollector } from "@alienplatform/platform-api/models";

let value: RenderOperatorManifestRequestLogCollector = {};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `enabled`                                                            | *boolean*                                                            | :heavy_minus_sign:                                                   | N/A                                                                  |
| `podLabelKey`                                                        | *string*                                                             | :heavy_minus_sign:                                                   | Existing Pod label key to collect logs from. Set with podLabelValue. |
| `podLabelValue`                                                      | *string*                                                             | :heavy_minus_sign:                                                   | Existing Pod label value to collect logs from. Set with podLabelKey. |
