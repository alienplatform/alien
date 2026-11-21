# GetSetupEnrollmentLocationUnion


## Supported Types

### `operations.GetSetupEnrollmentLocationCloudformation`

```typescript
const value: operations.GetSetupEnrollmentLocationCloudformation = {
  kind: "cloudformation",
  stackName: "<value>",
  region: "<value>",
};
```

### `operations.GetSetupEnrollmentLocationTerraform`

```typescript
const value: operations.GetSetupEnrollmentLocationTerraform = {
  kind: "terraform",
  cloud: "azure",
  resourceId: "<id>",
};
```
