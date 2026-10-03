import { ApolloServer } from '@apollo/server';
import { expressMiddleware } from '@as-integrations/express5';
import cors from 'cors';
import express from 'express';

const typeDefs = `#graphql
  type Query {
    hello: String!
  }
`;

const resolvers = {
  Query: {
    hello: () => 'Hello from the Apollo monorepo!',
  },
};

const server = new ApolloServer({ typeDefs, resolvers });
await server.start();

const app = express();
app.use('/graphql', cors(), express.json(), expressMiddleware(server));

const port = Number(process.env.PORT ?? 4000);
app.listen(port, () => {
  console.log(`Apollo Server ready at http://localhost:${port}/graphql`);
});
