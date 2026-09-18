# ManagerRetryResponseCompute2

Deployment-time compute choices for Alien-managed compute pools.

Application source declares portable pool requirements. This settings
object stores the concrete choices made for one deployment, such as the
provider machine type and selected machine counts.

## Example Usage

```typescript
import { ManagerRetryResponseCompute2 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseCompute2 = {};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `pools`                                                  | Record<string, *models.ManagerRetryResponsePoolsUnion2*> | :heavy_minus_sign:                                       | Selected compute choices keyed by pool ID.               |