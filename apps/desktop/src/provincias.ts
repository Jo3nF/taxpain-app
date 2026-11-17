/**
 * Provincias de España con su código del INE, que es también el de los modelos (190, 193…) y el
 * principio del código postal (46001 → 46, Valencia). Nombres oficiales del INE.
 */
export const PROVINCES: readonly (readonly [string, string])[] = [
  ['01', 'Araba/Álava'],
  ['02', 'Albacete'],
  ['03', 'Alicante/Alacant'],
  ['04', 'Almería'],
  ['05', 'Ávila'],
  ['06', 'Badajoz'],
  ['07', 'Illes Balears'],
  ['08', 'Barcelona'],
  ['09', 'Burgos'],
  ['10', 'Cáceres'],
  ['11', 'Cádiz'],
  ['12', 'Castellón/Castelló'],
  ['13', 'Ciudad Real'],
  ['14', 'Córdoba'],
  ['15', 'A Coruña'],
  ['16', 'Cuenca'],
  ['17', 'Girona'],
  ['18', 'Granada'],
  ['19', 'Guadalajara'],
  ['20', 'Gipuzkoa'],
  ['21', 'Huelva'],
  ['22', 'Huesca'],
  ['23', 'Jaén'],
  ['24', 'León'],
  ['25', 'Lleida'],
  ['26', 'La Rioja'],
  ['27', 'Lugo'],
  ['28', 'Madrid'],
  ['29', 'Málaga'],
  ['30', 'Murcia'],
  ['31', 'Navarra'],
  ['32', 'Ourense'],
  ['33', 'Asturias'],
  ['34', 'Palencia'],
  ['35', 'Las Palmas'],
  ['36', 'Pontevedra'],
  ['37', 'Salamanca'],
  ['38', 'Santa Cruz de Tenerife'],
  ['39', 'Cantabria'],
  ['40', 'Segovia'],
  ['41', 'Sevilla'],
  ['42', 'Soria'],
  ['43', 'Tarragona'],
  ['44', 'Teruel'],
  ['45', 'Toledo'],
  ['46', 'Valencia/València'],
  ['47', 'Valladolid'],
  ['48', 'Bizkaia'],
  ['49', 'Zamora'],
  ['50', 'Zaragoza'],
  ['51', 'Ceuta'],
  ['52', 'Melilla'],
];

/** La provincia de un código postal español (cinco cifras), o null si no lo es. */
export function provinceOfPostCode(postCode: string): string | null {
  const m = /^\s*(\d{2})\d{3}\s*$/.exec(postCode);
  return (m && PROVINCES.find(([c]) => c === m[1])?.[1]) || null;
}

/**
 * La provincia que toca al cambiar el código postal: la del nuevo si la que había estaba vacía o
 * era la del código anterior (puesta sola); si la escribió el usuario, se respeta.
 */
export function provinceAfterPostCode(
  oldPostCode: string,
  newPostCode: string,
  province: string,
): string {
  const next = provinceOfPostCode(newPostCode);
  if (!next) return province;
  const auto = !province.trim() || province === provinceOfPostCode(oldPostCode);
  return auto ? next : province;
}
