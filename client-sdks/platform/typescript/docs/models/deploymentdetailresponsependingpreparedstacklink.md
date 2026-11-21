# DeploymentDetailResponsePendingPreparedStackLink

What a workload that links a resource of this type gets.

The extension computes a link's environment variables from the resource's outputs, and
declares their names here, so a release can see two links that set the same variable. The
permission sets are fixed per type, because setup grants them before the resource has outputs.

## Example Usage

```typescript
import { DeploymentDetailResponsePendingPreparedStackLink } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponsePendingPreparedStackLink = {
  permissionSets: [
    "<value 1>",
  ],
};
```

## Fields

| Field                                                                                                       | Type                                                                                                        | Required                                                                                                    | Description                                                                                                 |
| ----------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| `env`                                                                                                       | *string*[]                                                                                                  | :heavy_minus_sign:                                                                                          | Names of the environment variables the workload gets. A link that sets any other name<br/>fails the deployment. |
| `permissionSets`                                                                                            | *string*[]                                                                                                  | :heavy_check_mark:                                                                                          | Ids of the permission sets the workload gets on the linked resource.                                        |