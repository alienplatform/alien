# HelmUpgrade

The chart a Helm install must upgrade to. Null until the chart is built.

## Example Usage

```typescript
import { HelmUpgrade } from "@alienplatform/platform-api/models";

let value: HelmUpgrade = {
  chart: "<value>",
  version: "<value>",
  releaseName: "<value>",
  namespace: "<value>",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `chart`                                                                    | *string*                                                                   | :heavy_check_mark:                                                         | OCI reference of the chart to upgrade to.                                  |
| `version`                                                                  | *string*                                                                   | :heavy_check_mark:                                                         | Chart version that carries this update.                                    |
| `releaseName`                                                              | *string*                                                                   | :heavy_check_mark:                                                         | Helm release name the install reported. Null until it reports.             |
| `namespace`                                                                | *string*                                                                   | :heavy_check_mark:                                                         | Namespace of the Helm release the install reported. Null until it reports. |