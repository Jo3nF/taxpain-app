//! Criptomonedas: cantidades exactas, precios y motor de cálculo (lotes, PMP/FIFO, resultados).

pub mod calculo;
pub mod qty;

pub use calculo::{
    Calculo, Gasto, Hueco, Ingreso, Inicial, Inicio, Lote, Metodo, Operacion, Posicion, Realizado,
    Resumen, SIN_PLATAFORMA, Tipo, calcular, calcular_desde, resumen, resumen_entre, valorar,
};
pub use qty::{Precio, Qty};
