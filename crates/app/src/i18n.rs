//! UI translation catalog. Keep English source strings as stable keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    PtBr,
    EnUs,
}
impl Language {
    pub fn parse(locale: &str) -> Self {
        if locale
            .to_ascii_lowercase()
            .replace('_', "-")
            .starts_with("pt")
        {
            Self::PtBr
        } else {
            Self::EnUs
        }
    }
    pub fn detect() -> Self {
        #[cfg(target_arch = "wasm32")]
        {
            let window = web_sys::window().unwrap();
            let stored = window
                .local_storage()
                .ok()
                .flatten()
                .and_then(|s| s.get_item("spacehunter.language").ok().flatten());
            Self::parse(
                &stored
                    .or_else(|| window.navigator().language())
                    .unwrap_or_default(),
            )
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(locale) =
                Self::preference_path().and_then(|p| std::fs::read_to_string(p).ok())
            {
                return Self::parse(locale.trim());
            }
            Self::parse(
                &std::env::var("SPACEHUNTER_LANG")
                    .or_else(|_| std::env::var("LC_ALL"))
                    .or_else(|_| std::env::var("LANG"))
                    .unwrap_or_default(),
            )
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn preference_path() -> Option<std::path::PathBuf> {
        std::env::var_os("APPDATA")
            .or_else(|| std::env::var_os("XDG_CONFIG_HOME"))
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|p| std::path::PathBuf::from(p).join(".config"))
            })
            .map(|p| p.join("spacehunter").join("language"))
    }
    pub fn save(self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(p) = Self::preference_path() {
            if let Some(parent) = p.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(p, if self == Self::PtBr { "pt-BR" } else { "en-US" });
        }
        #[cfg(target_arch = "wasm32")]
        if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            let _ = s.set_item(
                "spacehunter.language",
                if self == Self::PtBr { "pt-BR" } else { "en-US" },
            );
        }
    }
    pub fn text(self, key: &str) -> &str {
        if self == Self::EnUs {
            return key;
        }
        match key {
            "Overview" => "Visão geral",
            "All files" => "Todos os arquivos",
            "Click for details · Double-click to explore" => "Clique para ver detalhes · Clique duplo para explorar",
            "Other items" => "Outros itens",
            "Folder" => "Pasta",
            "File" => "Arquivo",
            "Double-click to see these items in All files." => "Clique duas vezes para ver estes itens em Todos os arquivos.",
            "Open folder" => "Escolher pasta",
            "Drives" => "Unidades",
            "Demo" => "Demonstração",
            "Rescan" => "Analisar novamente",
            "Scan again (F5)" => "Analisar novamente (F5)",
            "Full view" => "Visão completa",
            "Zoom full (Home)" => "Visão completa (Home)",
            "Parent folder" => "Pasta acima",
            "Zoom out (Backspace)" => "Voltar (Backspace)",
            "Zoom into folder" => "Ampliar pasta",
            "Free space" => "Espaço livre",
            "Open in file manager" => "Abrir no gerenciador de arquivos",
            "Delete" => "Excluir",
            "Setup" => "Configurações",
            "About" => "Sobre",
            "Panel" => "Painel",
            "Nothing loaded." => "Nenhuma pasta carregada.",
            "Size" => "Tamanho",
            "Of view" => "Da visualização",
            "Files" => "Arquivos",
            "Folders" => "Pastas",
            "Modified" => "Modificado",
            "CONTENTS OF THIS VIEW" => "CONTEÚDO DESTA VISUALIZAÇÃO",
            "LARGEST FILES IN VIEW" => "MAIORES ARQUIVOS",
            "SpaceHunter Setup" => "Configurações do Space Hunter",
            "File layout" => "Mapa de arquivos",
            "Algorithm:" => "Algoritmo:",
            "Classic (SpaceMonger)" => "Clássico (SpaceMonger)",
            "Squarified" => "Quadrificado",
            "Density:" => "Densidade:",
            "Bias:" => "Orientação:",
            "Vert" => "Vertical",
            "Horz" => "Horizontal",
            "Folder title bars" => "Títulos das pastas",
            "Display colors" => "Cores da visualização",
            "3D shaded blocks" => "Blocos com relevo 3D",
            "Show names inside blocks" => "Mostrar nomes nos blocos",
            "Dark theme" => "Tema escuro",
            "3D view" => "Visualização 3D",
            "Height proportional to file size" => "Altura proporcional ao tamanho do arquivo",
            "Height scale" => "Escala de altura",
            "Miscellaneous" => "Outras opções",
            "Animated zoom in / zoom out" => "Animar zoom",
            "Show tooltips" => "Mostrar dicas",
            "Measure size on disk (clusters/blocks) – applies on next scan" => "Medir tamanho em disco (clusters/blocos) – na próxima análise",
            "Enable the Delete command (permanent!)" => "Habilitar exclusão permanente",
            "About SpaceHunter" => "Sobre o Space Hunter",
            "A fast, modern take on the classic SpaceMonger disk-space visualiser." => "Um visualizador de espaço em disco rápido e moderno, inspirado no SpaceMonger.",
            "• Parallel Rust scanner, squarified treemap, 2D and 3D views" => "• Análise paralela em Rust, mapas em 2D e 3D",
            "• Double-click a folder to zoom in · Backspace to zoom out" => "• Clique duplo para entrar em uma pasta · Backspace para voltar",
            "• 3D: drag to orbit, right-drag to pan, wheel to zoom" => "• 3D: arraste para girar, botão direito para mover, roda para zoom",
            "Delete permanently?" => "Excluir permanentemente?",
            "Cancel" => "Cancelar",
            "Increase density in Setup, or zoom in to see them." => "Aumente a densidade nas configurações ou aproxime para ver os itens.",
            "Show in file manager" => "Mostrar no gerenciador de arquivos",
            "Copy path" => "Copiar caminho",
            "Delete…" => "Excluir…",
            "Nothing here" => "Nenhum item aqui",
            "Scanning" => "Analisando",
            "See where your disk space went." => "Descubra o que ocupa seu disco.",
            "Choose a folder or drive…" => "Escolher uma pasta ou unidade…",
            "Choose a folder…" => "Escolher uma pasta…",
            "QUICK SCAN" => "ANÁLISE RÁPIDA",
            "Try with demo data" => "Experimentar com dados de exemplo",
            "…or drop a folder onto this page.\nFiles never leave your device." => "…ou arraste uma pasta para esta página.\nSeus arquivos ficam no seu dispositivo.",
            "Ready." => "Pronto.",
            "By file type" => "Por tipo de arquivo",
            "Rainbow" => "Arco-íris",
            "By depth" => "Por profundidade",
            "Size heat-map" => "Mapa de calor por tamanho",
            "Monochrome" => "Monocromático",
            "Too many files" => "Arquivos em excesso",
            "Very many files" => "Muitos arquivos",
            "Lots of files" => "Vários arquivos",
            "Normal" => "Normal",
            "Very few files" => "Poucos arquivos",
            "Too few files" => "Pouquíssimos arquivos",
            "Choose a folder or drive (Ctrl+O)" => "Escolher pasta ou unidade (Ctrl+O)",
            "Choose a folder to analyse (Ctrl+O). Nothing is uploaded – everything stays in your browser." => "Escolher pasta para analisar (Ctrl+O). Tudo fica no seu navegador.",
            "Zoom into the selected folder (Enter / double-click)" => "Entrar na pasta selecionada (Enter / clique duplo)",
            "Show the free space of the drive as one more block" => "Mostrar espaço livre como um bloco",
            "Open with the default application / show in file manager" => "Abrir com aplicativo padrão / mostrar no gerenciador",
            "Disabled – enable it in Setup" => "Desativado – habilite nas configurações",
            "drag: orbit · right-drag: pan · wheel: zoom · double-click: enter folder" => "arraste: girar · botão direito: mover · roda: zoom · clique duplo: entrar",
            _ => key,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn locale_selection_and_translation() {
        assert_eq!(Language::parse("pt_BR"), Language::PtBr);
        assert_eq!(Language::parse("en-US"), Language::EnUs);
        assert_eq!(Language::parse("fr-FR"), Language::EnUs);
        assert_eq!(
            Language::PtBr.text("Choose a folder…"),
            "Escolher uma pasta…"
        );
        assert_eq!(Language::EnUs.text("Choose a folder…"), "Choose a folder…");
    }
}
