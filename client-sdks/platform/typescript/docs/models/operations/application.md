# Application

## Example Usage

```typescript
import { Application } from "@alienplatform/platform-api/models/operations";

let value: Application = {
  state: "up-to-date",
  reason: "<value>",
  reported: {
    releaseId: "<id>",
    version: "<value>",
  },
  desired: {
    releaseId: "<id>",
    version: "<value>",
    source: "pin",
    helmChart: {
      chart: "<value>",
      version: "<value>",
    },
    images: [],
  },
  releaseChannel: "<value>",
  rollback: {
    releaseId: "<id>",
    version: "<value>",
  },
};
```

## Fields

| Field                                                                                                                                                                          | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `state`                                                                                                                                                                        | [operations.ApplicationState](../../models/operations/applicationstate.md)                                                                                                     | :heavy_check_mark:                                                                                                                                                             | How the application release the Operator reports compares with the desired release. Alien never applies the change; the customer applies it through their own release process. |
| `reason`                                                                                                                                                                       | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | N/A                                                                                                                                                                            |
| `reported`                                                                                                                                                                     | [operations.ApplicationReported](../../models/operations/applicationreported.md)                                                                                               | :heavy_check_mark:                                                                                                                                                             | The application release the Operator reports                                                                                                                                   |
| `desired`                                                                                                                                                                      | [operations.Desired](../../models/operations/desired.md)                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | The release the deployment should run: its pinned release or its channel's release                                                                                             |
| `releaseChannel`                                                                                                                                                               | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | The release channel the deployment follows when unpinned                                                                                                                       |
| `rollback`                                                                                                                                                                     | [operations.Rollback](../../models/operations/rollback.md)                                                                                                                     | :heavy_check_mark:                                                                                                                                                             | The release the deployment reported before it moved to the desired release                                                                                                     |