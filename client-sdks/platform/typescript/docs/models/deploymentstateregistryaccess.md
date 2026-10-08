# DeploymentStateRegistryAccess

The cross-account read a manager opened on Alien's registry for one deployment.

## Example Usage

```typescript
import { DeploymentStateRegistryAccess } from "@alienplatform/platform-api/models";

let value: DeploymentStateRegistryAccess = {};
```

## Fields

| Field                                                                                                                            | Type                                                                                                                             | Required                                                                                                                         | Description                                                                                                                      |
| -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `repositories`                                                                                                                   | *string*[]                                                                                                                       | :heavy_minus_sign:                                                                                                               | Repository identifiers the grant names, sorted.                                                                                  |
| `serviceTypes`                                                                                                                   | *string*[]                                                                                                                       | :heavy_minus_sign:                                                                                                               | Compute services the grant admits, sorted. Each pulls as its own principal, so a service<br/>added later needs the policy rewritten. |