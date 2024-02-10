# Deploy contract to secret

```
#install requirements
cd secret-ibc-rng-template/node
npm i

#add mnemonid
nano .env
MNEMONIC="mnemonic here"

#build and deploy
cd ../proxy
make build-mainnet
make build-mainnet-reproducible
cd ../node
node upload_instantiate_secret_proxy
```
