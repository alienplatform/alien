# ObservedApplicationReport

Application release the Operator observes running in its environment.

This identifies the customer's application, not the Operator: the
Operator's own image is reported separately as [`OperatorImageReport`].

## Example Usage

```typescript
import { ObservedApplicationReport } from "@alienplatform/manager-api/models";

let value: ObservedApplicationReport = {
  complete: false,
  observedAt: new Date("2024-02-13T15:59:34.490Z"),
  source: "kubernetes",
};
```

## Fields

| Field                                                                                                                                | Type                                                                                                                                 | Required                                                                                                                             | Description                                                                                                                          |
| ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| `chartName`                                                                                                                          | *string*                                                                                                                             | :heavy_minus_sign:                                                                                                                   | Helm chart name from the workloads' `helm.sh/chart` label. Present only<br/>when every labelled workload names the same chart.       |
| `chartVersion`                                                                                                                       | *string*                                                                                                                             | :heavy_minus_sign:                                                                                                                   | Helm chart version from the same label.                                                                                              |
| `complete`                                                                                                                           | *boolean*                                                                                                                            | :heavy_check_mark:                                                                                                                   | Whether every workload kind could be listed. When `false`, the chart<br/>and images describe only the workloads the Operator could read. |
| `images`                                                                                                                             | [models.ObservedApplicationImage](../models/observedapplicationimage.md)[]                                                           | :heavy_minus_sign:                                                                                                                   | Distinct container images running in the observed workloads.                                                                         |
| `observedAt`                                                                                                                         | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                                        | :heavy_check_mark:                                                                                                                   | When the workloads were read.                                                                                                        |
| `source`                                                                                                                             | [models.ObservedApplicationSource](../models/observedapplicationsource.md)                                                           | :heavy_check_mark:                                                                                                                   | Where the Operator read the application identity from.                                                                               |
