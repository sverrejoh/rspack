import { value } from './used';
import './unused';
import './style.css';
import { cycleValue, useHelper } from './cycle-a';
import { layered } from './layered';
import externalModule from 'external-module';

it('should build the graph the snapshot describes', () => {
  expect(value).toBe(42);
  expect(cycleValue).toBe(1);
  expect(useHelper()).toBe(1);
  expect(layered).toBe('layered');
  expect(typeof externalModule.join).toBe('function');
});
