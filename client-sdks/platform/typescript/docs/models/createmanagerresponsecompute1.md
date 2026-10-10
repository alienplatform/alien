# CreateManagerResponseCompute1

Deployment-time compute choices for Alien-managed compute pools.

Application source declares portable pool requirements. This settings
object stores the concrete choices made for one deployment, such as the
provider machine type and selected machine counts.

## Example Usage

```typescript
import { CreateManagerResponseCompute1 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseCompute1 = {};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `containers`                                                                                             | Record<string, [models.CreateManagerResponseContainers1](../models/createmanagerresponsecontainers1.md)> | :heavy_minus_sign:                                                                                       | Per-replica resources selected within each container's declared ranges.                                  |
| `pools`                                                                                                  | Record<string, *models.CreateManagerResponsePoolsUnion1*>                                                | :heavy_minus_sign:                                                                                       | Selected compute choices keyed by pool ID.                                                               |