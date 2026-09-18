# CreateManagerResponseNetworkUnion2


## Supported Types

### `models.CreateManagerResponseNetworkUseDefault2`

```typescript
const value: models.CreateManagerResponseNetworkUseDefault2 = {
  type: "use-default",
};
```

### `models.CreateManagerResponseNetworkCreate2`

```typescript
const value: models.CreateManagerResponseNetworkCreate2 = {
  type: "create",
};
```

### `models.CreateManagerResponseNetworkByoVpcAws2`

```typescript
const value: models.CreateManagerResponseNetworkByoVpcAws2 = {
  privateSubnetIds: [],
  publicSubnetIds: [],
  type: "byo-vpc-aws",
  vpcId: "<id>",
};
```

### `models.CreateManagerResponseNetworkByoVpcGcp2`

```typescript
const value: models.CreateManagerResponseNetworkByoVpcGcp2 = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

### `models.CreateManagerResponseNetworkByoVnetAzure2`

```typescript
const value: models.CreateManagerResponseNetworkByoVnetAzure2 = {
  privateSubnetName: "<value>",
  publicSubnetName: "<value>",
  type: "byo-vnet-azure",
  vnetResourceId: "<id>",
};
```

### `any`

```typescript
const value: any = "<value>";
```

