# CreateProjectHelmRequest

Helm chart package configuration. If null, Helm packages will not be generated.

## Example Usage

```typescript
import { CreateProjectHelmRequest } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectHelmRequest = {
  chartName: "<value>",
  description: "late ouch deceivingly behest and unfinished bashfully all",
  enabled: false,
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `chartName`                                                                                                            | *string*                                                                                                               | :heavy_check_mark:                                                                                                     | Chart name (e.g., "acme-operator")                                                                                     |
| `description`                                                                                                          | *string*                                                                                                               | :heavy_check_mark:                                                                                                     | Human-friendly description of the chart                                                                                |
| `logCollector`                                                                                                         | [operations.CreateProjectLogCollectorRequest](../../models/operations/createprojectlogcollectorrequest.md)             | :heavy_minus_sign:                                                                                                     | Default log collection mode in a generated Helm chart.                                                                 |
| `enabled`                                                                                                              | *boolean*                                                                                                              | :heavy_check_mark:                                                                                                     | Whether Helm chart package generation is enabled                                                                       |
| `setupResources`                                                                                                       | [operations.CreateProjectSetupResourcesRequest](../../models/operations/createprojectsetupresourcesrequest.md)         | :heavy_minus_sign:                                                                                                     | N/A                                                                                                                    |
| `runtimePersistence`                                                                                                   | [operations.CreateProjectRuntimePersistenceRequest](../../models/operations/createprojectruntimepersistencerequest.md) | :heavy_minus_sign:                                                                                                     | N/A                                                                                                                    |