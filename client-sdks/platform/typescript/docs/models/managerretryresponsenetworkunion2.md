# ManagerRetryResponseNetworkUnion2


## Supported Types

### `models.ManagerRetryResponseNetworkUseDefault2`

```typescript
const value: models.ManagerRetryResponseNetworkUseDefault2 = {
  type: "use-default",
};
```

### `models.ManagerRetryResponseNetworkCreate2`

```typescript
const value: models.ManagerRetryResponseNetworkCreate2 = {
  type: "create",
};
```

### `models.ManagerRetryResponseNetworkByoVpcAws2`

```typescript
const value: models.ManagerRetryResponseNetworkByoVpcAws2 = {
  privateSubnetIds: [
    "<value 1>",
  ],
  publicSubnetIds: [
    "<value 1>",
    "<value 2>",
  ],
  type: "byo-vpc-aws",
  vpcId: "<id>",
};
```

### `models.ManagerRetryResponseNetworkByoVpcGcp2`

```typescript
const value: models.ManagerRetryResponseNetworkByoVpcGcp2 = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

### `models.ManagerRetryResponseNetworkByoVnetAzure2`

```typescript
const value: models.ManagerRetryResponseNetworkByoVnetAzure2 = {
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

