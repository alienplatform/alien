# Kept

Deployment-time compute choices for Alien-managed compute pools.

Application source declares portable pool requirements. This settings
object stores the concrete choices made for one deployment, such as the
provider machine type and selected machine counts.

## Example Usage

```typescript
import { Kept } from "@alienplatform/platform-api/models";

let value: Kept = {};
```

## Fields

| Field                                                                   | Type                                                                    | Required                                                                | Description                                                             |
| ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| `containers`                                                            | Record<string, [models.KeptContainers](../models/keptcontainers.md)>    | :heavy_minus_sign:                                                      | Per-replica resources selected within each container's declared ranges. |
| `pools`                                                                 | Record<string, *models.KeptPoolsUnion*>                                 | :heavy_minus_sign:                                                      | Selected compute choices keyed by pool ID.                              |