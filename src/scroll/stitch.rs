#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewRegion {
    pub top: u32,
    pub height: u32,
}

#[derive(Debug)]
pub struct PreviewPatch<'a> {
    pub document_width: u32,
    pub document_height: u32,
    pub region: PreviewRegion,
    pub rgba: &'a [u8],
}

#[derive(Debug)]
pub struct OwnedPreviewPatch {
    pub document_width: u32,
    pub document_height: u32,
    pub region: PreviewRegion,
    pub rgba: Vec<u8>,
}

impl OwnedPreviewPatch {
    pub fn as_patch(&self) -> PreviewPatch<'_> {
        PreviewPatch {
            document_width: self.document_width,
            document_height: self.document_height,
            region: self.region,
            rgba: &self.rgba,
        }
    }
}
