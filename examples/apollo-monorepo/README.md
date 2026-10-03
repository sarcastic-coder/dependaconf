# Apollo Server + Apollo Client monorepo

This npm-workspaces example contains an Apollo Server 5 API in
`packages/server` and an Apollo Client 4 + React app in `packages/client`.
Start the API and client in separate terminals from this directory:

```sh
npm run dev:server
npm run dev:client
```

The API listens at `http://localhost:4000/graphql`; the Vite client displays
the API's greeting at the development URL it prints. Build both workspaces with
`npm run build`.

The checked-in [`.github/dependabot.yml`](.github/dependabot.yml) is generated
from the root `package-lock.json` and includes separate update entries for the
root and both workspaces. Each entry groups only dependencies declared by its
own package. From the repository root, regenerate it with:

```sh
cd examples/apollo-monorepo
cargo run --manifest-path ../../Cargo.toml
```

No npm install is needed just to regenerate the Dependabot configuration.
