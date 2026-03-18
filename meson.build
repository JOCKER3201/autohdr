project('vk_layer_hdr_tensor', 'c', version: '0.1.0')

cargo = find_program('cargo', required: true)

cargo_target_dir = meson.current_build_dir() / 'cargo_target'
manifest_path = meson.project_source_root() / 'Cargo.toml'

# Wywołujemy cargo do zbudowania biblioteki Rust i kopiujemy plik .so do katalogu budowania Meson
shared_lib = custom_target(
  'vk_layer_hdr_tensor_so',
  build_by_default: true,
  build_always_stale: true,
  output: 'libVkLayer_hdr_tensor.so',
  console: true,
  command: [
    'sh', '-c',
    'CARGO_TARGET_DIR="$1" cargo build --release --manifest-path "$2" && cp "$1/release/libVkLayer_hdr_tensor.so" "$3"',
    '--',
    cargo_target_dir, manifest_path, '@OUTPUT@'
  ],
  install: true,
  install_dir: get_option('libdir')
)

# Konfigurujemy plik JSON warstwy do użytku w katalogu budowania
conf_data_build = configuration_data()
conf_data_build.set('LAYER_PATH', meson.current_build_dir() / 'libVkLayer_hdr_tensor.so')

configure_file(
  input: 'layer.json.in',
  output: 'VkLayer_hdr_tensor.json',
  configuration: conf_data_build
)

# Konfigurujemy plik JSON warstwy do instalacji (z właściwą ścieżką docelową)
conf_data_install = configuration_data()
# get_option('prefix') / get_option('libdir') / 'libVkLayer_hdr_tensor.so'
conf_data_install.set('LAYER_PATH', get_option('prefix') / get_option('libdir') / 'libVkLayer_hdr_tensor.so')

configure_file(
  input: 'layer.json.in',
  output: 'VkLayer_hdr_tensor_install.json',
  configuration: conf_data_install,
  install: true,
  install_dir: get_option('datadir') / 'vulkan' / 'explicit_layer.d',
  install_tag: 'runtime'
)

# Zmieniamy nazwę zainstalowanego pliku na właściwą
meson.add_install_script('sh', '-c', 'mv "$MESON_INSTALL_DESTDIR_PREFIX/$1/VkLayer_hdr_tensor_install.json" "$MESON_INSTALL_DESTDIR_PREFIX/$1/VkLayer_hdr_tensor.json"',
  '--', get_option('datadir') / 'vulkan' / 'explicit_layer.d'
)
