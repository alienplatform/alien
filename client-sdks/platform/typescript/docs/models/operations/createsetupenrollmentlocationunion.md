# CreateSetupEnrollmentLocationUnion


## Supported Types

### `operations.CreateSetupEnrollmentLocationCloudformation`

```typescript
const value: operations.CreateSetupEnrollmentLocationCloudformation = {
  kind: "cloudformation",
  stackName: "<value>",
  region: "<value>",
};
```

### `operations.CreateSetupEnrollmentLocationTerraform`

```typescript
const value: operations.CreateSetupEnrollmentLocationTerraform = {
  kind: "terraform",
  cloud: "aws",
  resourceId: "<id>",
};
```
