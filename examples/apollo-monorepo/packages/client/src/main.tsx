import { ApolloClient, gql, HttpLink, InMemoryCache } from '@apollo/client';
import { ApolloProvider, useQuery } from '@apollo/client/react';
import { createRoot } from 'react-dom/client';

const client = new ApolloClient({
  link: new HttpLink({ uri: 'http://localhost:4000/graphql' }),
  cache: new InMemoryCache(),
});

function Greeting() {
  const { loading, error, data } = useQuery<{ hello: string }>(gql`
    query Greeting {
      hello
    }
  `);

  if (error) return <p>Could not reach the API: {error.message}</p>;
  if (loading || !data) return <p>Loading...</p>;
  return <p>{data.hello}</p>;
}

createRoot(document.getElementById('root')!).render(
  <ApolloProvider client={client}>
    <Greeting />
  </ApolloProvider>,
);
