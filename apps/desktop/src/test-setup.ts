// Configuración común de Vitest. En máquinas lentas (el Mac Intel de GitHub, o el Mac propio
// con mucha carga) un `findBy…` o `waitFor` puede tardar más de 1 s sin que nada vaya mal: se
// espera hasta 5 s. Sólo cambia cuánto se espera a que pase algo, no lo que se comprueba.
import { configure } from '@testing-library/react';

configure({ asyncUtilTimeout: 5000 });
