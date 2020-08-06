use crate::voxel::{setter::SetVoxelsRequest, Voxel, VoxelMap};

use amethyst::{core::ecs::prelude::*, shrev::EventChannel};
use ilattice3 as lat;
use ilattice3::VecLatticeMap;
use std::collections::{HashMap, HashSet};

#[cfg(feature = "profiler")]
use thread_profiler::profile_scope;

/// Used by systems that want to double buffer their `SetVoxelRequests`, allowing them to run in
/// concurrently with the `VoxelSetterSystem`. Any requests written here in frame T will be written
/// to the `VoxelMap` at the end of frame T+1.
#[derive(Default)]
pub struct VoxelSetRequestsBackBuffer {
    pub requests: Vec<SetVoxelsRequest>,
}

/// For the sake of pipelining, all voxels edits are first written out of place by the
/// `VoxelSetterSystem`. They get merged into the `VoxelMap` by the `VoxelDoubleBufferingSystem` at
/// the end of a frame.
#[derive(Default)]
pub struct VoxelEditsBackBuffer {
    pub edited_chunks: HashMap<lat::Point, VecLatticeMap<Voxel>>,
    pub neighbor_chunks: Vec<lat::Point>,
}

#[derive(Default)]
pub struct DirtyChunks {
    pub chunks: HashSet<lat::Point>,
}

/// The system responsible for merging the `VoxelEditsBackBuffer` into the `VoxelMap`. This allows the
/// `VoxelChunkReloaderSystem` and `VoxelSetterSystem` to run in parallel.
pub struct VoxelDoubleBufferingSystem;

impl<'a> System<'a> for VoxelDoubleBufferingSystem {
    type SystemData = (
        Write<'a, VoxelSetRequestsBackBuffer>,
        Write<'a, EventChannel<SetVoxelsRequest>>,
        Write<'a, Option<VoxelEditsBackBuffer>>,
        Write<'a, Option<DirtyChunks>>,
        WriteExpect<'a, VoxelMap>,
    );

    fn run(
        &mut self,
        (
            mut set_requests, mut set_voxels_channel, mut edits, mut dirty_chunks, mut map
        ): Self::SystemData,
    ) {
        #[cfg(feature = "profiler")]
        profile_scope!("voxel_double_buffering");

        // Submit the requests to the setter.
        set_voxels_channel.drain_vec_write(&mut set_requests.requests);

        // Merge the edits into the map.
        let VoxelEditsBackBuffer {
            edited_chunks,
            neighbor_chunks,
        } = edits.take().unwrap();
        let mut new_dirty_chunks = HashSet::new();
        for (chunk_key, chunk_voxels) in edited_chunks.into_iter() {
            map.voxels.map.insert_chunk(chunk_key, chunk_voxels);
            new_dirty_chunks.insert(chunk_key);
        }
        new_dirty_chunks.extend(neighbor_chunks.into_iter());

        // Update the set of dirty chunks so the `ChunkReloaderSystem` can see them on the next
        // frame.
        assert!(dirty_chunks.is_none());
        *dirty_chunks = Some(DirtyChunks {
            chunks: new_dirty_chunks,
        });
    }
}