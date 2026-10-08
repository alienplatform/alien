# DeploymentSetupScaffolding

Cloud objects a direct setup created so a runtime-owned resource can run.

## Example Usage

```typescript
import { DeploymentSetupScaffolding } from "@alienplatform/platform-api/models";

let value: DeploymentSetupScaffolding = {
  buildRoleName: "<value>",
  type: "awsSandbox",
};
```

## Fields

| Field                                                                                                       | Type                                                                                                        | Required                                                                                                    | Description                                                                                                 |
| ----------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| `buildRoleName`                                                                                             | *string*                                                                                                    | :heavy_check_mark:                                                                                          | IAM role the image build runs as.                                                                           |
| `egress`                                                                                                    | *models.DeploymentEgressUnion*                                                                              | :heavy_minus_sign:                                                                                          | N/A                                                                                                         |
| `imageArn`                                                                                                  | *string*                                                                                                    | :heavy_minus_sign:                                                                                          | A Frozen sandbox's MicroVM image, built during setup. A Live one's image belongs to its<br/>runtime controller. |
| `type`                                                                                                      | [models.DeploymentTypeAwsSandbox](../models/deploymenttypeawssandbox.md)                                    | :heavy_check_mark:                                                                                          | N/A                                                                                                         |