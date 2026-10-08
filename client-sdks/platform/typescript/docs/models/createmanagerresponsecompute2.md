# CreateManagerResponseCompute2

Deployment-time compute choices for Alien-managed compute pools.

Application source declares portable pool requirements. This settings
object stores the concrete choices made for one deployment, such as the
provider machine type and selected machine counts.

## Example Usage

```typescript
import { CreateManagerResponseCompute2 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseCompute2 = {};
```

## Fields

| Field                                                     | Type                                                      | Required                                                  | Description                                               |
| --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- |
| `pools`                                                   | Record<string, *models.CreateManagerResponsePoolsUnion2*> | :heavy_minus_sign:                                        | Selected compute choices keyed by pool ID.                |