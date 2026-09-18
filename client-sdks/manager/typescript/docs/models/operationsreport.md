# OperationsReport

Report-only summary of the operations the Operator has loaded, used to
confirm a bundle sync actually took effect.

## Example Usage

```typescript
import { OperationsReport } from "@alienplatform/manager-api/models";

let value: OperationsReport = {};
```

## Fields

| Field                                                                                                                                 | Type                                                                                                                                  | Required                                                                                                                              | Description                                                                                                                           |
| ------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| `loadedBundleHash`                                                                                                                    | *string*                                                                                                                              | :heavy_minus_sign:                                                                                                                    | Hash of the enabled-plugin bundle set the Operator currently has<br/>loaded. Compared against the platform's target hash to detect drift. |
| `operations`                                                                                                                          | [models.ReportedOperation](../models/reportedoperation.md)[]                                                                          | :heavy_minus_sign:                                                                                                                    | Operations currently loaded and executable by the Operator.                                                                           |