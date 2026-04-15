import { IdGenerator } from './src/index.ts';

const MAX_TIMESTAMP = 128_063_081_718_015;

const gen = new IdGenerator();
const id1 = gen.nextIdAt(MAX_TIMESTAMP);
console.log(`At MAX: ${id1}`);

try {
  const id2 = gen.nextIdAt(MAX_TIMESTAMP + 1);
  console.log(`ERROR: ${id2}`);
} catch (e: any) {
  console.log(`OK: ${e.message}`);
}
